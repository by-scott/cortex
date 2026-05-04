# Replay Migration 语料

Cortex 的 replay claim 依赖 journaled events、projection versions、replay
diffs、side-effect substitution 和受检查的 replay fixtures。Replay Migration
语料是一个 release-review fixture，用来记录在提出 migration 或 compatibility
claim 前需要检查哪些 replay 表面。

语料位置：

```text
scenarios/replay-migration/corpus.json
```

当前语料引用仓库内已有 replay fixtures：

```text
crates/cortex-kernel/tests/fixtures/replay/externalized_compaction_boundary.toml
crates/cortex-kernel/tests/fixtures/replay/tool_effect_transaction.toml
```

这是 Eval/Scenario 证据。它不能证明每个历史 release 的 journal 或 database
都能不经审查迁移成功。没有运行 historical snapshot 时，release evidence
必须明确说明。

## Review 路径

对 release candidate，先运行正常 Docker gate 和行为证据命令，再在 release
evidence template 中附上语料 review：

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

当前 replay 证据主要来自：

```bash
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_fixture_corpus_projects_current_surfaces
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_diff_reports_projection_changes
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_side_effect_substitution_prefers_provider_values
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay journal_replay_keeps_guardrail_and_external_input_events_stable
```

语料本身由 `replay_migration_corpus_is_parseable_and_documented` contract test
检查可解析性和文档覆盖。

## 必需 Review 记录

每个 case 都要记录：

- 已审查的 fixture path 或 replay test surface；
- source 和 target release scope；
- projection surface 和 expected evidence；
- 是否附上 replay diff 与 deterministic digest 证据；
- 只运行 current fixture 时的 limitation。

不要把未运行的历史迁移标成 passed。只有实际运行 candidate 的 historical
fixtures、journals 或 database snapshots 后，才能写成 historical migration
通过。
