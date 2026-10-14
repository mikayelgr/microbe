// Typed wrappers around the Rust commands. Types come from ts-rs bindings.
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Activity as McpActivity } from "./bindings/Activity";
import type { AnalysisRequest } from "./bindings/AnalysisRequest";
import type { AnalysisResult } from "./bindings/AnalysisResult";
import type { BoardFirmware } from "./bindings/BoardFirmware";
import type { BuilderInfo } from "./bindings/BuilderInfo";
import type { BuildRequest } from "./bindings/BuildRequest";
import type { BuildResult } from "./bindings/BuildResult";
import type { BoardIndex } from "./bindings/BoardIndex";
import type { Candidate } from "./bindings/Candidate";
import type { ConnectOptions } from "./bindings/ConnectOptions";
import type { DeviceEntry } from "./bindings/DeviceEntry";
import type { DeviceStatus } from "./bindings/DeviceStatus";
import type { Endpoint } from "./bindings/Endpoint";
import type { FlashRequest } from "./bindings/FlashRequest";
import type { FlashResult } from "./bindings/FlashResult";
import type { InstallOptions } from "./bindings/InstallOptions";
import type { InstallReport } from "./bindings/InstallReport";
import type { LocalEntry } from "./bindings/LocalEntry";
import type { CheckInfo } from "./bindings/CheckInfo";
import type { Finding } from "./bindings/Finding";
import type { Instrumented } from "./bindings/Instrumented";
import type { Partition } from "./bindings/Partition";
import type { PartitionTable } from "./bindings/PartitionTable";
import type { TestCase } from "./bindings/TestCase";
import type { TestFile } from "./bindings/TestFile";
import type { LspProviderInfo } from "./bindings/LspProviderInfo";
import type { McpOptions } from "./bindings/McpOptions";
import type { McpStatus } from "./bindings/McpStatus";
import type { PackageInfo } from "./bindings/PackageInfo";
import type { RescueOptions } from "./bindings/RescueOptions";
import type { RescueReport } from "./bindings/RescueReport";
import type { PlatformProfile } from "./bindings/PlatformProfile";
import type { Prelude } from "./bindings/Prelude";
import type { ProgressEvent } from "./bindings/ProgressEvent";
import type { SessionEnvelope } from "./bindings/SessionEnvelope";
import type { StubRequest } from "./bindings/StubRequest";
import type { StubSet } from "./bindings/StubSet";
import type { SyncOptions } from "./bindings/SyncOptions";
import type { SyncReport } from "./bindings/SyncReport";
import type { TermEvent } from "./bindings/TermEvent";
import type { TransportInfo } from "./bindings/TransportInfo";

export type {
  AnalysisRequest, AnalysisResult, CheckInfo, Finding, Instrumented, Partition, PartitionTable, McpActivity, McpOptions, McpStatus, TestCase, TestFile,
  BoardFirmware, BuilderInfo, BuildRequest, BuildResult, BoardIndex, Candidate, ConnectOptions, DeviceEntry, DeviceStatus, Endpoint,
  FlashRequest, FlashResult, InstallOptions, InstallReport, LocalEntry, LspProviderInfo,
  PackageInfo, PlatformProfile, RescueOptions, RescueReport, Prelude, ProgressEvent, SessionEnvelope, StubRequest, StubSet,
  SyncOptions, SyncReport, TermEvent, TransportInfo,
};

export const api = {
  // device
  transports: () => invoke<TransportInfo[]>("transports_list"),
  platforms: () => invoke<PlatformProfile[]>("platforms_list"),
  discover: (transport?: string) => invoke<Endpoint[]>("device_discover", { transport }),
  connect: (endpoint: Endpoint, options: ConnectOptions, interrupt: boolean, term: Channel<TermEvent>) =>
    invoke<DeviceStatus>("device_connect", { endpoint, options, interrupt, term }),
  identify: (sessionId: string) => invoke<DeviceStatus>("device_identify", { sessionId }),
  sessions: () => invoke<DeviceStatus[]>("device_sessions"),
  disconnect: (sessionId: string) => invoke<void>("device_disconnect", { sessionId }),
  write: (sessionId: string, data: string) => invoke<void>("device_write", { sessionId, data }),
  interrupt: (sessionId: string) => invoke<void>("device_interrupt", { sessionId }),
  softReset: (sessionId: string) => invoke<void>("device_soft_reset", { sessionId }),
  hardReset: (sessionId: string) => invoke<void>("device_hard_reset", { sessionId }),
  run: (sessionId: string, code: string, preludes?: Prelude[]) =>
    invoke<void>("device_run", { sessionId, code, preludes }),
  stop: (sessionId: string, force = false) => invoke<void>("device_stop", { sessionId, force }),
  exec: (sessionId: string, code: string) => invoke<string>("device_exec", { sessionId, code }),
  script: (sessionId: string, script: string, args: Record<string, string | number | boolean> = {}) =>
    invoke<string>("device_script", { sessionId, script, args }),
  bleHelperInstall: (sessionId: string, name: string) => invoke<void>("ble_helper_install", { sessionId, name }),
  rescue: (sessionId: string, options: RescueOptions) => invoke<RescueReport>("device_rescue", { sessionId, options }),

  // device files
  fsList: (sessionId: string, path: string) => invoke<DeviceEntry[]>("fs_list", { sessionId, path }),
  fsReadText: (sessionId: string, path: string) => invoke<string>("fs_read_text", { sessionId, path }),
  fsWriteText: (sessionId: string, path: string, content: string) =>
    invoke<void>("fs_write_text", { sessionId, path, content }),
  fsUpload: (sessionId: string, localPaths: string[], remoteDir: string) =>
    invoke<string[]>("fs_upload", { sessionId, localPaths, remoteDir }),
  fsDownload: (sessionId: string, remotePath: string, localPath: string) =>
    invoke<void>("fs_download", { sessionId, remotePath, localPath }),
  fsMkdir: (sessionId: string, path: string) => invoke<void>("fs_mkdir", { sessionId, path }),
  fsRemove: (sessionId: string, path: string) => invoke<void>("fs_remove", { sessionId, path }),
  fsRename: (sessionId: string, src: string, dst: string) => invoke<void>("fs_rename", { sessionId, src, dst }),
  sync: (sessionId: string, options: SyncOptions) => invoke<SyncReport>("project_sync", { sessionId, options }),

  // local project
  projectList: (dir: string) => invoke<LocalEntry[]>("project_list", { dir }),
  projectRead: (path: string) => invoke<string>("project_read", { path }),
  projectWrite: (path: string, content: string) => invoke<void>("project_write", { path, content }),
  projectCreate: (path: string, isDir: boolean) => invoke<void>("project_create", { path, isDir }),
  projectRemove: (path: string) => invoke<void>("project_remove", { path }),
  projectRename: (src: string, dst: string) => invoke<void>("project_rename", { src, dst }),

  // firmware
  firmwareIndex: (force = false) => invoke<BoardIndex>("firmware_index", { force }),
  firmwareBoard: (boardId: string) => invoke<BoardFirmware>("firmware_board", { boardId }),
  firmwareDetect: (deep: boolean) => invoke<Candidate[]>("firmware_detect", { deep }),
  firmwareFlash: (request: FlashRequest) => invoke<FlashResult>("firmware_flash", { request }),
  esp32Partitions: (sessionId: string) => invoke<PartitionTable>("esp32_partitions", { sessionId }),
  esp32ImagePartitions: (path: string) => invoke<PartitionTable>("esp32_image_partitions", { path }),
  firmwareBuilders: () => invoke<BuilderInfo>("firmware_builders"),
  firmwareBuild: (request: BuildRequest, log: Channel<string>) => invoke<BuildResult>("firmware_build", { request, log }),
  firmwareBuildCancel: () => invoke<void>("firmware_build_cancel"),

  // packages
  packagesIndex: (force = false) => invoke<PackageInfo[]>("packages_index", { force }),
  packagesInstall: (sessionId: string, options: InstallOptions) =>
    invoke<InstallReport>("packages_install", { sessionId, options }),

  // language server
  lspProviders: () => invoke<LspProviderInfo[]>("lsp_providers"),
  stubsEnsure: (request: StubRequest) => invoke<StubSet>("stubs_ensure", { request }),
  lspStart: (provider: string, stubs: StubSet | null, channel: Channel<string>) =>
    invoke<void>("lsp_start", { provider, stubs, channel }),
  lspSend: (message: string) => invoke<void>("lsp_send", { message }),
  lspSetStubs: (stubs: StubSet | null) => invoke<void>("lsp_set_stubs", { stubs }),
  lspStop: () => invoke<void>("lsp_stop"),

  // static analysis
  analysisChecks: () => invoke<CheckInfo[]>("analysis_checks"),
  analysisRun: (request: AnalysisRequest) => invoke<AnalysisResult>("analysis_run", { request }),

  // tests and debugging
  testsDiscover: (root: string) => invoke<TestFile[]>("tests_discover", { root }),
  debugInstrument: (source: string) => invoke<Instrumented>("debug_instrument", { source }),

  // MCP server
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  mcpStart: (options: McpOptions) => invoke<McpStatus>("mcp_start", { options }),
  mcpStop: () => invoke<McpStatus>("mcp_stop"),
  mcpRegenerateToken: () => invoke<McpStatus>("mcp_regenerate_token"),
};

export const events = {
  session: (cb: (e: SessionEnvelope) => void): Promise<UnlistenFn> =>
    listen<SessionEnvelope>("session", (e) => cb(e.payload)),
  progress: (cb: (e: ProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ProgressEvent>("progress", (e) => cb(e.payload)),
  notice: (cb: (msg: string) => void): Promise<UnlistenFn> => listen<string>("notice", (e) => cb(e.payload)),
  lsp: (cb: (e: { kind: string }) => void): Promise<UnlistenFn> =>
    listen<{ kind: string }>("lsp", (e) => cb(e.payload)),
  mcp: (cb: (e: McpActivity) => void): Promise<UnlistenFn> => listen<McpActivity>("mcp", (e) => cb(e.payload)),
};

export { Channel };

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
