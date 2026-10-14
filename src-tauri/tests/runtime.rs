//! Runtime features on the structured channel, end to end on the unix port:
//! debugger, test runner, asyncio inspector, heap dump.
#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{records, run_finished, start, wait_for};
use microbe_lib::{debug, scripts, testing};
use serde_json::{json, Value};

fn render(h: &common::Harness, script: &str, args: Value) -> String {
    let args: BTreeMap<String, Value> = serde_json::from_value(args).unwrap();
    scripts::render_json(&h.session.profile(), script, &args).unwrap()
}

async fn next_record(h: &common::Harness, tag: &str, n: usize) -> Value {
    wait_for(h, || records(h, tag).len() > n).await;
    records(h, tag)[n].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn debugger_breakpoints_stepping_and_eval() {
    let Some(h) = start() else { return };
    let src = "def add(a, b):\n    s = a + b\n    return s\n\ntotal = 0\nfor i in range(3):\n    total = add(total, i)\nprint('total', total)\n";
    let inst = debug::instrument(src).unwrap();
    let prelude = render(
        &h,
        "debugger",
        json!({ "breakpoints": "[2]", "functions": serde_json::to_string(&inst.functions).unwrap() }),
    );
    h.session.run(inst.code, vec![prelude]).await.unwrap();

    let stop = next_record(&h, "DBG", 0).await;
    assert_eq!(stop["line"], json!(2));
    assert_eq!(stop["reason"], json!("breakpoint"));
    assert_eq!(stop["stack"], json!(["<module>", "add"]));
    assert_eq!(stop["locals"]["a"], json!("0"));
    assert!(stop["locals"].get("s").is_none(), "s isn't bound yet");
    assert_eq!(stop["globals"]["total"], json!("0"));

    h.session.write(b"n\r").await.unwrap();
    let stop = next_record(&h, "DBG", 1).await;
    assert_eq!((stop["line"].clone(), stop["locals"]["s"].clone()), (json!(3), json!("0")));

    // Step out: the next line in the caller (next loop iteration).
    h.session.write(b"o\r").await.unwrap();
    let stop = next_record(&h, "DBG", 2).await;
    assert_eq!(stop["line"], json!(7));
    assert_eq!(stop["stack"], json!(["<module>"]));

    h.session.write(b"e i * 10\r").await.unwrap();
    let ev = next_record(&h, "DBG", 3).await;
    assert_eq!((ev["event"].clone(), ev["result"].clone()), (json!("eval"), json!("10")));

    // Clear breakpoints and run to the end.
    h.session.write(b"b\r").await.unwrap();
    h.session.write(b"c\r").await.unwrap();
    assert_eq!(run_finished(&h).await, None);
    assert_eq!(records(&h, "DBG").len(), 4);
    assert!(String::from_utf8_lossy(&h.term.lock().unwrap()).contains("total 3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn debugger_pauses_a_running_program() {
    let Some(h) = start() else { return };
    let src = "import time\nn = 0\nwhile True:\n    n += 1\n    time.sleep_ms(1)\n";
    let inst = debug::instrument(src).unwrap();
    let prelude = render(&h, "debugger", json!({ "breakpoints": "[]", "functions": "[\"<module>\"]" }));
    h.session.run(inst.code, vec![prelude]).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(records(&h, "DBG").is_empty());
    h.session.write(b"p\r").await.unwrap();
    let stop = next_record(&h, "DBG", 0).await;
    assert_eq!(stop["reason"], json!("pause"));
    assert!(stop["globals"]["n"].as_str().unwrap().parse::<u32>().unwrap() > 0);
    // A breakpoint set while running takes effect.
    h.session.write(b"c\r").await.unwrap();
    h.session.write(b"b 5\r").await.unwrap();
    let stop = next_record(&h, "DBG", 1).await;
    assert_eq!((stop["line"].clone(), stop["reason"].clone()), (json!(5), json!("breakpoint")));
    h.session.stop(false).await.unwrap();
    assert!(run_finished(&h).await.unwrap().contains("KeyboardInterrupt"));
}

#[tokio::test(flavor = "multi_thread")]
async fn debugger_stops_are_skipped_in_tracebacks() {
    let Some(h) = start() else { return };
    let src = "x = 1\ny = x / 0\n";
    let inst = debug::instrument(src).unwrap();
    let prelude = render(&h, "debugger", json!({ "breakpoints": "[]", "functions": "[\"<module>\"]" }));
    h.session.run(inst.code, vec![prelude]).await.unwrap();
    let err = run_finished(&h).await.unwrap();
    assert!(err.contains("ZeroDivisionError"), "{err}");
    let out = String::from_utf8_lossy(&h.term.lock().unwrap()).into_owned();
    assert!(out.contains("line 2"), "line numbers survive instrumentation: {out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_runner_reports_each_test() {
    let Some(h) = start() else { return };
    let src = "def test_ok():\n    assert 1 + 1 == 2\n\ndef test_fail():\n    assert 1 == 2, 'nope'\n\nclass TestThing:\n    def setUp(self):\n        self.v = 5\n    def test_v(self):\n        assert self.v == 5\n    def test_err(self):\n        raise ValueError('boom')\n\nasync def test_async():\n    import asyncio\n    await asyncio.sleep_ms(1)\n";
    let names: Vec<String> = testing::find_tests(src).unwrap().into_iter().map(|t| t.name).collect();
    let code = format!("{src}\n__mb_tests.run(globals(), {})\n", serde_json::to_string(&names).unwrap());
    h.session.run(code, vec![render(&h, "testrunner", json!({}))]).await.unwrap();
    assert_eq!(run_finished(&h).await, None);

    let results: Vec<(String, String)> = records(&h, "TEST")
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["status"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        results,
        vec![
            ("test_ok".into(), "pass".into()),
            ("test_fail".into(), "fail".into()),
            ("TestThing.test_v".into(), "pass".into()),
            ("TestThing.test_err".into(), "error".into()),
            ("test_async".into(), "pass".into()),
        ]
    );
    let fail = &records(&h, "TEST")[1];
    assert_eq!(fail["message"], json!("nope"));
    let err = &records(&h, "TEST")[3];
    assert!(err["traceback"].as_str().unwrap().contains("ValueError: boom"));
    assert_eq!(records(&h, "TESTS")[0], json!({ "passed": 3, "failed": 1, "errors": 1, "skipped": 0 }));
}

#[tokio::test(flavor = "multi_thread")]
async fn asyncio_inspector_reports_tasks_and_cleans_up() {
    let Some(h) = start() else { return };
    let code = "import asyncio\nasync def sleeper():\n    await asyncio.sleep_ms(150)\nasync def main():\n    asyncio.create_task(sleeper())\n    await asyncio.sleep_ms(100)\nasyncio.run(main())\n";
    let args = json!({ "period_ms": 20 });
    let prelude = render(&h, "aio_watch", args.clone());
    let cleanup = render(&h, "aio_watch_end", args);
    h.session.run_with(code.into(), vec![prelude], vec![cleanup]).await.unwrap();
    assert_eq!(run_finished(&h).await, None);
    let snaps = records(&h, "TASKS");
    assert!(snaps.len() >= 2, "{snaps:?}");
    let names: Vec<&str> = snaps[1]["tasks"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"main") && names.contains(&"sleeper"), "{names:?}");
    assert!(snaps[1]["tasks"].as_array().unwrap().iter().any(|t| t["state"] == json!("sleeping") && t["wake_ms"].is_number()));

    // The cleanup restored asyncio.
    let mut raw = h.session.raw().await.unwrap();
    let out = raw.exec_str("import asyncio\nprint(hasattr(asyncio.core, '__mb_orig'))").await;
    assert_eq!(raw.finish_with(out).await.unwrap().trim(), "False");
}

#[tokio::test(flavor = "multi_thread")]
async fn heap_dump_and_memory_sampler() {
    let Some(h) = start() else { return };
    let heap = render(&h, "heap", json!({}));
    let mut raw = h.session.raw().await.unwrap();
    let out = raw.exec_str(&heap).await;
    let out = raw.finish_with(out).await.unwrap();
    assert!(out.contains("GC: total:") && out.contains("GC memory layout"), "{out}");

    // The unix port has no machine.Timer: the sampler stays quiet and the
    // program and its cleanup still run.
    let args = json!({ "period_ms": 50 });
    let (p, c) = (render(&h, "mem_watch", args.clone()), render(&h, "mem_watch_end", args));
    h.session.run_with("print('ran')".into(), vec![p], vec![c]).await.unwrap();
    assert_eq!(run_finished(&h).await, None);
    assert!(String::from_utf8_lossy(&h.term.lock().unwrap()).contains("ran"));
}

#[tokio::test(flavor = "multi_thread")]
async fn stubs_generated_from_the_board() {
    let Some(h) = start() else { return };
    // A stub set that only knows a few modules.
    let cache = tempfile::tempdir().unwrap();
    let base_dir = cache.path().join("sets").join("unix-any-latest");
    std::fs::create_dir_all(base_dir.join("stdlib")).unwrap();
    std::fs::write(base_dir.join("stdlib/sys.pyi"), "").unwrap();
    std::fs::write(base_dir.join("stdlib/VERSIONS"), "sys: 3.0-\njson: 3.0-\nos: 3.0-\n").unwrap();
    let base = microbe_lib::lsp::stubs::StubSet {
        key: "unix-any-latest".into(),
        typeshed: base_dir.display().to_string(),
        packages: vec!["micropython-stdlib-stubs==1".into()],
        version: "1".into(),
    };
    let set = microbe_lib::lsp::introspect::extend(&h.session, &base, cache.path()).await.unwrap();
    assert_ne!(set.key, base.key);
    assert!(set.packages.last().unwrap().starts_with("from the board: "), "{:?}", set.packages);
    let stdlib = std::path::Path::new(&set.typeshed).join("stdlib");
    let uctypes = std::fs::read_to_string(stdlib.join("uctypes.pyi")).unwrap();
    assert!(uctypes.contains("def sizeof(*args: Any, **kwargs: Any) -> Any: ..."), "{uctypes}");
    assert!(uctypes.contains("UINT8: int"));
    let versions = std::fs::read_to_string(stdlib.join("VERSIONS")).unwrap();
    assert!(versions.contains("uctypes: 3.0-") && versions.contains("sys: 3.0-"));
    assert!(!stdlib.join("json.pyi").exists(), "covered modules are left alone");
    assert!(!stdlib.join("__main__.pyi").exists() && !stdlib.join("_thread.pyi").exists());
    // Cached: the second call doesn't regenerate.
    let again = microbe_lib::lsp::introspect::extend(&h.session, &base, cache.path()).await.unwrap();
    assert_eq!(again.key, set.key);
}

/// A stand-in for the ESP32 `esp32` module on the unix port: partitions are
/// files in the sandbox.
const FAKE_ESP32: &str = r#"
class Partition:
    RUNNING = 1
    BOOT = 0
    TYPE_APP = 0
    TYPE_DATA = 1
    def __init__(self, which, size=65536, label="ota_0", sub=16, off=65536):
        self.label, self.size, self.sub, self.off = label, size, sub, off
    def info(self):
        return (0, self.sub, self.off, self.size, self.label, False)
    def get_next_update(self):
        return Partition(None, label="ota_1", sub=17, off=65536 * 2)
    @staticmethod
    def find(t):
        return [Partition(None)] if t == 0 else []
    def ioctl(self, op, arg):
        return 4096
    def _f(self, mode):
        return open(self.label + ".img", mode)
    def writeblocks(self, n, buf):
        assert len(buf) % 4096 == 0
        try:
            data = bytearray(self._f("rb").read())
        except OSError:
            data = bytearray()
        end = (n + 1) * 4096
        if len(data) < end:
            data.extend(b"\xff" * (end - len(data)))
        data[n * 4096:end] = buf
        f = self._f("wb"); f.write(data); f.close()
    def readblocks(self, n, buf):
        f = self._f("rb"); f.seek(n * 4096); buf[:] = f.read(4096); f.close()
    def set_boot(self):
        f = open("boot.txt", "w"); f.write(self.label); f.close()
"#;

async fn ota_job_and_run(h: &common::Harness, image: &std::path::Path, chip: Option<&str>) -> microbe_lib::error::Result<()> {
    let image = image.to_path_buf();
    {
        use microbe_lib::firmware::flashers::{ota::Esp32Ota, FlashJob, Flasher};
        let platforms = microbe_lib::platform::PlatformRegistry::load(None).unwrap();
        let esp = platforms.get("esp32").unwrap();
        let method = esp.flash.iter().find(|m| m.method == "esp32_ota").unwrap();
        let reporter = microbe_lib::progress::Reporter::with_sink(Arc::new(|_| {}), "ota", "OTA");
        let target = microbe_lib::firmware::detect::FlashTarget::Session { session_id: h.session.id.clone() };
        let http = reqwest::Client::new();
        let cache = tempfile::tempdir().unwrap();
        let profile = h.session.profile();
        let job = FlashJob {
            method,
            profile: &profile,
            firmware: &image,
            target: &target,
            chip: chip.as_deref(),
            erase: false,
            reporter: &reporter,
            http: &http,
            cache: cache.path(),
            session: Some(h.session.clone()),
        };
        Esp32Ota.flash(job).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn esp32_ota_writes_verifies_and_switches_partition() {
    let Some(h) = start() else { return };
    std::fs::write(h.dir.path().join("esp32.py"), FAKE_ESP32).unwrap();
    // An app image for an ESP32-S3 (chip id 9), 10000 bytes: three blocks.
    let mut app = vec![0xE9u8, 4, 2, 0x20, 0, 0, 0, 0, 0xEE, 0, 0, 0, 9, 0];
    app.extend((0..10000 - 14).map(|i| (i * 31 % 251) as u8));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fw.app-bin");
    std::fs::write(&path, &app).unwrap();

    // Wrong chip: refused before anything is written.
    let err = ota_job_and_run(&h, &path, Some("ESP32C3")).await.unwrap_err();
    assert!(err.to_string().contains("different chip"), "{err}");
    assert!(!h.dir.path().join("ota_1.img").exists());

    ota_job_and_run(&h, &path, Some("esp32s3")).await.unwrap();
    let written = std::fs::read(h.dir.path().join("ota_1.img")).unwrap();
    assert_eq!(&written[..app.len()], &app[..]);
    assert!(written[app.len()..].iter().all(|&b| b == 0xFF), "last block padded with 0xFF");
    assert_eq!(std::fs::read_to_string(h.dir.path().join("boot.txt")).unwrap(), "ota_1");

    // The partition table as the board reports it.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let code = scripts::render(&h.session.profile(), "partitions", &[]).unwrap();
    let mut raw = h.session.raw().await.unwrap();
    let out = raw.exec_str(&code).await;
    let table = microbe_lib::firmware::esp32::parse_board_table(&raw.finish_with(out).await.unwrap()).unwrap();
    assert_eq!((table.running.as_deref(), table.next_update.as_deref()), (Some("ota_0"), Some("ota_1")));
}

#[tokio::test(flavor = "multi_thread")]
async fn esp32_ota_refuses_images_that_do_not_fit() {
    let Some(h) = start() else { return };
    std::fs::write(h.dir.path().join("esp32.py"), FAKE_ESP32).unwrap();
    let mut app = vec![0xE9u8; 16];
    app.resize(70000, 0); // the fake partition holds 65536
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.app-bin");
    std::fs::write(&path, &app).unwrap();
    let err = ota_job_and_run(&h, &path, None).await.unwrap_err();
    assert!(err.to_string().contains("doesn't fit"), "{err}");
    assert!(!h.dir.path().join("boot.txt").exists());
}
