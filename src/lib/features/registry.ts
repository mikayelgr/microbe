// Sidebar features. Each feature is a panel plus the capabilities it needs;
// adding one is a new folder and a line here.
import type { Component } from "svelte";
import FilesPanel from "./files/FilesPanel.svelte";
import DevicePanel from "./device/DevicePanel.svelte";
import PackagesPanel from "./packages/PackagesPanel.svelte";
import FirmwarePanel from "./firmware/FirmwarePanel.svelte";
import SettingsPanel from "./settings/SettingsPanel.svelte";
import TestsPanel from "./tests/TestsPanel.svelte";
import DebugPanel from "./debug/DebugPanel.svelte";

export interface Feature {
  id: string;
  title: string;
  icon: string;
  panel: Component;
  /** Capabilities the connected board must have; absent = always available. */
  requires?: string[];
  /** Pinned to the bottom of the activity bar. */
  bottom?: boolean;
}

export const features: Feature[] = [
  { id: "files", title: "Project", icon: "files", panel: FilesPanel },
  { id: "device", title: "Board", icon: "chip", panel: DevicePanel },
  { id: "debug", title: "Debug", icon: "bug", panel: DebugPanel },
  { id: "tests", title: "Tests", icon: "tests", panel: TestsPanel },
  { id: "packages", title: "Packages", icon: "package", panel: PackagesPanel },
  { id: "firmware", title: "Firmware", icon: "bolt", panel: FirmwarePanel },
  { id: "settings", title: "Settings", icon: "gear", panel: SettingsPanel, bottom: true },
];

export function register(f: Feature) {
  features.push(f);
}

export function isAvailable(f: Feature, caps: string[] | undefined): boolean {
  return !f.requires?.length || (!!caps && f.requires.every((c) => caps.includes(c)));
}
