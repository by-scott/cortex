# Actor Leakage 语料

Cortex 将 actor ownership 作为 sessions、memory、tasks、goals、retrieval
evidence、channels、transports 和 operator surfaces 的运行时边界。Actor
Leakage 语料是一个小型 release-review fixture，用来检查 requester 是否能
通过普通接口观察或修改其他 actor 的状态。

语料位置：

```text
scenarios/actor-leakage/corpus.json
```

这是 Eval/Scenario 证据。它不是沙箱隔离，不是 hostile multi-tenant
hardening，也不能证明所有 transport、plugin 或 deployment mode 都已经拥有
完整 actor isolation。

## Review 路径

对 release candidate，先运行正常 Docker gate 和行为证据命令，再在 release
evidence template 中附上语料 review：

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

当前 actor-boundary 证据主要来自 runtime 和 memory contract suites：

```bash
docker compose run --rm dev cargo test -p cortex-runtime daemon_sessions --all-features
docker compose run --rm dev cargo test -p cortex-runtime rpc_sessions --all-features
docker compose run --rm dev cargo test -p cortex-runtime rpc_memory --all-features
docker compose run --rm dev cargo test -p cortex-turn --test memory_tools --all-features
```

语料本身由 `actor_leakage_corpus_is_parseable_and_documented` contract test
检查可解析性和文档覆盖。

## 必需 Review 记录

每个 case 都要记录：

- 哪个 actor 发起访问，哪个 actor 拥有目标状态；
- list/get/search/update/cancel routes 是否过滤或拒绝 hidden id；
- transport rebinding 是否保留历史 ownership；
- non-local actor 是否被挡在 local-operator surface 之外；
- 如果只做了静态证据 review，要把 limitation 写清楚。

不要把未审阅的语料标成 passed。缺少 active exploit run 时，必须继续作为
limitation 暴露。
