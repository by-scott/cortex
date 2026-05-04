# Prompt-Injection 语料

Cortex 将外部文本保持为 evidence，而不是 runtime instruction。Prompt
Injection 语料是一个小型 release review fixture，用来检查 web、file、
retrieval、plugin、channel 和 tool-output 表面上的这条边界。

语料位置：

```text
scenarios/prompt-injection/corpus.json
```

这是 Eval/Scenario 证据，不是完整 prompt-injection 防御，不是沙箱隔离，
也不能证明所有 hostile document 都会被阻断。它记录 release reviewer
应该检查和扩展的 case。

## Review 路径

对 release candidate，先运行正常 Docker gate 和行为证据命令，再在 release
evidence template 中附上语料 review：

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

现有 safety tests 覆盖运行时 guardrail 行为：

```bash
docker compose run --rm dev cargo test -p cortex-turn --test safety_contracts --all-features
```

语料本身由 `prompt_injection_corpus_is_parseable_and_documented` contract test
检查可解析性和文档覆盖。

## 必需 Review 记录

每个 case 都要记录：

- hostile payload 是否停留在 evidence/tool-result 平面；
- actor ownership、protected root、plugin trust 和 permission gate 是否仍是权威边界；
- 是否加入了 candidate-specific hostile evidence；
- 如果只做了静态证据 review，要把 limitation 写清楚。

不要把未审阅的语料标成 passed。缺少 active exploit run 时，必须继续作为
limitation 暴露。
