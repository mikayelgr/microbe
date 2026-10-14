//! Diagnostics against the real machine's devices. Not run by default:
//! `cargo test --test hardware -- --ignored --nocapture`

use microbe_lib::firmware::detect::{boot_volumes, DetectContext, DetectorRegistry};
use microbe_lib::platform::PlatformRegistry;
use microbe_lib::transport::{serial::SerialProvider, TransportProvider};

#[tokio::test]
#[ignore]
async fn print_detected_boards() {
    let platforms = PlatformRegistry::load(None).unwrap();
    let ports = SerialProvider.discover(&platforms).await.unwrap();
    println!("serial ports: {:#?}", ports.iter().map(|p| (&p.address, &p.label)).collect::<Vec<_>>());
    println!("candidate boot volumes: {:#?}", boot_volumes());
    let ctx = DetectContext { platforms: &platforms, sessions: vec![], serial_ports: ports, index: None, deep: false };
    let found = DetectorRegistry::with_defaults().detect(&ctx).await;
    println!("detected: {found:#?}");
}

/// Downloads the generic stub set and checks what it contains (network).
#[tokio::test]
#[ignore]
async fn generic_stubs_have_core_modules_only() {
    let dir = tempfile::tempdir().unwrap();
    let req = microbe_lib::lsp::stubs::StubRequest { port: "generic".into(), board: None, version: None, introspect_session: None };
    let set = microbe_lib::lsp::stubs::ensure(&reqwest::Client::new(), dir.path(), &req).await.unwrap();
    let stdlib = std::path::Path::new(&set.typeshed).join("stdlib");
    for m in ["machine.pyi", "time.pyi", "gc.pyi", "micropython.pyi", "asyncio"] {
        assert!(stdlib.join(m).exists(), "missing {m}");
    }
    assert!(!stdlib.join("rp2").exists(), "port-only module rp2 leaked into generic stubs");
    assert!(!stdlib.join("esp32.pyi").exists(), "port-only module esp32 leaked into generic stubs");
    println!("{set:#?}");
}

/// Builds the unix port from source in Docker (needs a running Docker daemon
/// and network; takes a few minutes).
#[tokio::test]
#[ignore]
async fn docker_builds_the_unix_port() {
    use microbe_lib::firmware::builder::{plan, BuildRequest, Builders, Docker};
    let docker = Docker::find().unwrap();
    docker.check().await.unwrap();
    let work = tempfile::tempdir().unwrap();
    let req = BuildRequest { port: "unix".into(), board: "standard".into(), variant: None, version: "1.24.1".into(), freeze_project: None, c_modules: None };
    let p = plan(&Builders::builtin().unwrap(), &req, work.path(), "hw").unwrap();
    let r = docker.build(&p, &|l| println!("{l}"), &tokio_util::sync::CancellationToken::new()).await.unwrap();
    println!("{r:#?}");
    assert!(r.artifacts.iter().any(|a| a.ends_with("/micropython")));
}
