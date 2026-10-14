// UI icons: Lucide (https://lucide.dev), imported per icon so only these ship.
// Components refer to icons by these short names via <Icon name="…" />.
import type { Component } from "svelte";
import type { IconProps } from "@lucide/svelte";
import Activity from "@lucide/svelte/icons/activity";
import ArrowDownToDot from "@lucide/svelte/icons/arrow-down-to-dot";
import ArrowUpFromDot from "@lucide/svelte/icons/arrow-up-from-dot";
import BugPlay from "@lucide/svelte/icons/bug-play";
import Cable from "@lucide/svelte/icons/cable";
import Circle from "@lucide/svelte/icons/circle";
import CircleAlert from "@lucide/svelte/icons/circle-alert";
import CircleCheck from "@lucide/svelte/icons/circle-check";
import CircleDot from "@lucide/svelte/icons/circle-dot";
import CircleSlash from "@lucide/svelte/icons/circle-slash";
import CircleX from "@lucide/svelte/icons/circle-x";
import Container from "@lucide/svelte/icons/container";
import HardDrive from "@lucide/svelte/icons/hard-drive";
import ListChecks from "@lucide/svelte/icons/list-checks";
import Network from "@lucide/svelte/icons/network";
import RedoDot from "@lucide/svelte/icons/redo-dot";
import Bluetooth from "@lucide/svelte/icons/bluetooth";
import Bot from "@lucide/svelte/icons/bot";
import Bug from "@lucide/svelte/icons/bug";
import ChartLine from "@lucide/svelte/icons/chart-line";
import Check from "@lucide/svelte/icons/check";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import Cpu from "@lucide/svelte/icons/cpu";
import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
import Copy from "@lucide/svelte/icons/copy";
import Download from "@lucide/svelte/icons/download";
import File from "@lucide/svelte/icons/file";
import FilePlus from "@lucide/svelte/icons/file-plus";
import Files from "@lucide/svelte/icons/files";
import FlaskConical from "@lucide/svelte/icons/flask-conical";
import Folder from "@lucide/svelte/icons/folder";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import FolderPlus from "@lucide/svelte/icons/folder-plus";
import FolderSync from "@lucide/svelte/icons/folder-sync";
import Hammer from "@lucide/svelte/icons/hammer";
import Info from "@lucide/svelte/icons/info";
import LifeBuoy from "@lucide/svelte/icons/life-buoy";
import MemoryStick from "@lucide/svelte/icons/memory-stick";
import Microchip from "@lucide/svelte/icons/microchip";
import Package from "@lucide/svelte/icons/package";
import Pencil from "@lucide/svelte/icons/pencil";
import Play from "@lucide/svelte/icons/play";
import Plug from "@lucide/svelte/icons/plug";
import Plus from "@lucide/svelte/icons/plus";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
import Save from "@lucide/svelte/icons/save";
import Search from "@lucide/svelte/icons/search";
import Settings from "@lucide/svelte/icons/settings";
import Sparkles from "@lucide/svelte/icons/sparkles";
import Square from "@lucide/svelte/icons/square";
import SquareTerminal from "@lucide/svelte/icons/square-terminal";
import Trash from "@lucide/svelte/icons/trash";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import Unplug from "@lucide/svelte/icons/unplug";
import Upload from "@lucide/svelte/icons/upload";
import Usb from "@lucide/svelte/icons/usb";
import Wifi from "@lucide/svelte/icons/wifi";
import X from "@lucide/svelte/icons/x";
import Zap from "@lucide/svelte/icons/zap";

export const UI_ICONS = {
  activity: Activity,
  "bug-play": BugPlay,
  cable: Cable,
  container: Container,
  dot: CircleDot,
  drive: HardDrive,
  network: Network,
  "step-into": ArrowDownToDot,
  "step-out": ArrowUpFromDot,
  "step-over": RedoDot,
  "test-pass": CircleCheck,
  "test-fail": CircleX,
  "test-error": CircleAlert,
  "test-skip": CircleSlash,
  "test-pending": Circle,
  tests: ListChecks,
  alert: TriangleAlert,
  bluetooth: Bluetooth,
  bolt: Zap,
  bug: Bug,
  chart: ChartLine,
  bot: Bot,
  check: Check,
  chevron: ChevronRight,
  chip: Microchip,
  copy: Copy,
  collapse: ChevronsDownUp,
  cpu: Cpu,
  download: Download,
  edit: Pencil,
  file: File,
  "file-plus": FilePlus,
  files: Files,
  flask: FlaskConical,
  folder: Folder,
  "folder-open": FolderOpen,
  "folder-plus": FolderPlus,
  gear: Settings,
  hammer: Hammer,
  info: Info,
  lifebuoy: LifeBuoy,
  memory: MemoryStick,
  package: Package,
  play: Play,
  plug: Plug,
  plus: Plus,
  refresh: RefreshCw,
  reset: RotateCcw,
  save: Save,
  search: Search,
  sparkle: Sparkles,
  stop: Square,
  sync: FolderSync,
  terminal: SquareTerminal,
  trash: Trash,
  unplug: Unplug,
  upload: Upload,
  usb: Usb,
  wifi: Wifi,
  x: X,
} satisfies Record<string, Component<IconProps>>;

export type UiIconName = keyof typeof UI_ICONS;
