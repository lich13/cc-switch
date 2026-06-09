export function releaseTagFromVersion(version: string): string {
  const trimmed = version.trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("v")) return trimmed;

  const forkVersion = trimmed.match(/^(\d+\.\d+\.\d+)-(\d+)$/);
  if (forkVersion) {
    return `v${forkVersion[1]}-lich13.${forkVersion[2]}`;
  }

  return `v${trimmed}`;
}
