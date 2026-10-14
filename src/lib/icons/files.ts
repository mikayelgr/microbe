// File and folder icons from the Material Icon Theme (the VS Code extension's
// npm package), resolved with the same rules VS Code applies to its manifest.
// Named imports so the bundler drops tables we don't use (e.g. the 160 KB
// folderNamesExpanded, which is always folderNames + "-open").
import {
  iconDefinitions,
  file,
  folder,
  folderExpanded,
  rootFolder,
  rootFolderExpanded,
  fileNames,
  fileExtensions,
  folderNames,
  rootFolderNames,
  rootFolderNamesExpanded,
  light,
} from "material-icon-theme/dist/material-icons.json";

type Table = Record<string, string>;
type LightTables = Partial<Record<"fileNames" | "fileExtensions" | "folderNames" | "folderNamesExpanded", Table>>;
const defs = iconDefinitions as Record<string, { iconPath: string }>;
const tables = { fileNames, fileExtensions, folderNames } as Record<"fileNames" | "fileExtensions" | "folderNames", Table>;
const lightTables = light as LightTables;

/** MicroPython-specific additions on top of the theme. */
const EXTRA_EXTENSIONS: Table = { mpy: "python-misc" };

const urls: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>("/node_modules/material-icon-theme/icons/*.svg", { eager: true, query: "?url", import: "default" }),
  ).map(([path, url]) => [path.split("/").pop()!.replace(/\.svg$/, ""), url]),
);

export interface IconQuery {
  name: string;
  isDir: boolean;
  open?: boolean;
  root?: boolean;
  light?: boolean;
}

function lookup(table: keyof typeof tables, key: string, isLight: boolean): string | undefined {
  return (isLight ? lightTables[table]?.[key] : undefined) ?? tables[table][key];
}

function openFolderIcon(closed: string, key: string, isLight: boolean): string {
  const fromLight = isLight ? lightTables.folderNamesExpanded?.[key] : undefined;
  if (fromLight) return fromLight;
  return defs[`${closed}-open`] ? `${closed}-open` : folderExpanded;
}

/** The theme's icon id for a file or folder. */
export function iconId({ name, isDir, open = false, root = false, light: isLight = false }: IconQuery): string {
  const n = name.toLowerCase();
  if (isDir) {
    if (root) return ((open ? rootFolderNamesExpanded : rootFolderNames) as Table)[n] ?? (open ? rootFolderExpanded : rootFolder);
    const closed = lookup("folderNames", n, isLight);
    if (!closed) return open ? folderExpanded : folder;
    return open ? openFolderIcon(closed, n, isLight) : closed;
  }
  const byName = lookup("fileNames", n, isLight);
  if (byName) return byName;
  // Longest compound extension first: "app.test.py" → "test.py", then "py".
  const parts = n.split(".");
  for (let i = 1; i < parts.length; i++) {
    const ext = parts.slice(i).join(".");
    const hit = lookup("fileExtensions", ext, isLight) ?? EXTRA_EXTENSIONS[ext];
    if (hit) return hit;
  }
  return file;
}

/** URL of the SVG for an icon id. */
export function iconUrl(id: string): string | undefined {
  const svg = defs[id]?.iconPath.split("/").pop()?.replace(/\.svg$/, "") ?? id;
  return urls[svg] ?? urls[file];
}
