# Winget golden fixtures

Real `winget list --id <X> --exact --accept-source-agreements` outputs captured
from the dev machine (winget v1.29.380, locale en-US, 2026-11-08) so the
list parser is tested against observed truth, not guessed mocks (ADR action #7).

| File | Case | Exit code |
|---|---|---|
| `winget_list_git_present.txt` | `Git.Git` installed | 0 |
| `winget_list_missing.txt` | `__EstzKit.Nonexistent.Package__` absent | 20 |

Notes:
- Not-found exits **20** on this winget version (not 1) and prints
  `No installed package found matching input criteria.`
- Row layout: `Name Id Version Available Source` (space-separated; `Name`
  may contain spaces, so the parser locates the `Id` token and takes the
  next token as the installed version).
- Output is locale-dependent; fixtures document en-US. Parsing never
  relies on the English "not found" sentence alone (exit code is primary).
