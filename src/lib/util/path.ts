// Path helpers. Board paths are always POSIX; local paths use the host's
// separator (inferred from the path itself).

export function sepOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

export function joinPath(dir: string, name: string): string {
  const sep = sepOf(dir);
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

export function parentPath(path: string): string {
  const sep = sepOf(path);
  const trimmed = path.length > 1 && path.endsWith(sep) ? path.slice(0, -1) : path;
  const i = trimmed.lastIndexOf(sep);
  if (i < 0) return ".";
  if (i === 0) return sep;
  return trimmed.slice(0, i);
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Python module name for a board path: "./lib/foo.py" → "lib.foo". */
export function moduleName(path: string): string {
  return path.replace(/^\.?\//, "").replace(/\.py$/, "").replace(/\//g, ".");
}
