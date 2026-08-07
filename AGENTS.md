<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **KasDev** (6612 symbols, 14169 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/KasDev/context` | Codebase overview, check index freshness |
| `gitnexus://repo/KasDev/clusters` | All functional areas |
| `gitnexus://repo/KasDev/processes` | All execution flows |
| `gitnexus://repo/KasDev/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |
| Work in the Web area (482 symbols) | `.claude/skills/generated/web/SKILL.md` |
| Work in the Scripts area (188 symbols) | `.claude/skills/generated/scripts/SKILL.md` |
| Work in the Tests area (187 symbols) | `.claude/skills/generated/tests/SKILL.md` |
| Work in the Examples area (48 symbols) | `.claude/skills/generated/examples/SKILL.md` |
| Work in the Py area (48 symbols) | `.claude/skills/generated/py/SKILL.md` |
| Work in the Cluster_140 area (44 symbols) | `.claude/skills/generated/cluster-140/SKILL.md` |
| Work in the Cluster_179 area (41 symbols) | `.claude/skills/generated/cluster-179/SKILL.md` |
| Work in the Js area (40 symbols) | `.claude/skills/generated/js/SKILL.md` |
| Work in the Cluster_194 area (29 symbols) | `.claude/skills/generated/cluster-194/SKILL.md` |
| Work in the Cluster_206 area (29 symbols) | `.claude/skills/generated/cluster-206/SKILL.md` |
| Work in the Cluster_189 area (20 symbols) | `.claude/skills/generated/cluster-189/SKILL.md` |
| Work in the Cluster_195 area (16 symbols) | `.claude/skills/generated/cluster-195/SKILL.md` |
| Work in the Traffic area (16 symbols) | `.claude/skills/generated/traffic/SKILL.md` |
| Work in the Node area (16 symbols) | `.claude/skills/generated/node/SKILL.md` |
| Work in the Cluster_151 area (13 symbols) | `.claude/skills/generated/cluster-151/SKILL.md` |
| Work in the Cluster_171 area (13 symbols) | `.claude/skills/generated/cluster-171/SKILL.md` |
| Work in the Cluster_185 area (13 symbols) | `.claude/skills/generated/cluster-185/SKILL.md` |
| Work in the Cluster_201 area (12 symbols) | `.claude/skills/generated/cluster-201/SKILL.md` |
| Work in the Cluster_209 area (11 symbols) | `.claude/skills/generated/cluster-209/SKILL.md` |
| Work in the Cluster_134 area (10 symbols) | `.claude/skills/generated/cluster-134/SKILL.md` |

<!-- gitnexus:end -->
