# 兼容性策略

Cortex 1.5 是完整重写。兼容性边界以本发布线里真实存在的代码和测试为准，
不是旧的 1.4 daemon surface。

## 1.5 稳定面

- tenant、actor、client、session、turn、event、delivery、permission、
  corpus 的类型化标识。
- `OwnedScope` 可见性规则，以及默认拒绝的跨 owner 检查。
- 按 visibility 过滤的文件 Journal replay。
- tenants、clients、sessions、memory、permission、delivery outbox、token
  usage 的 SQLite migrations。
- RAG query-scope authorization、corpus ACL、BM25 scoring、taint blocking、
  placement。
- SDK plugin manifest 的 ABI 校验和 declared-capability conformance。
- `scripts/gate.sh` 的 release gate 行为，以及
  `scripts/package-release.sh` 的资产命名。

## 带版本契约

- `cortex-sdk` ABI version。
- SQLite schema migration number。
- `cortex-types` 的公开 DTO。
- 发布包命名：`cortex-v${VERSION}-${PLATFORM}.tar.gz`。

这些 surface 变化必须有测试和 release note。

## 尚不稳定

Live daemon management、HTTP API、channel pairing、browser integration、native
plugin loading 还不是 1.5 重写 surface。它们必须在严格 gate 下重建后才能
重新写成可用功能。
