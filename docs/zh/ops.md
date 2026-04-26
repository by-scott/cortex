# 运维

## 当前 CLI

```bash
cortex version
cortex status
cortex release-plan
cortex help
```

`status` 报告 1.5 重写 surface：strict gate、多用户 ownership、journal
recovery、RAG、plugin authorization。

`release-plan` 输出发布顺序：

1. backup
2. migrate
3. install
4. smoke-test
5. package
6. publish

## Gate

发布 gate：

```bash
./scripts/gate.sh --docker
```

Docker Hub 不可达时，可以在已有 gate 镜像内运行同一套检查：

```bash
docker run --rm -e CORTEX_GATE_IN_DOCKER=1 \
  -v cortex-gate-cargo:/home/dev/.cargo \
  -v "$PWD":/workspace -w /workspace \
  cortex-gate:latest ./scripts/gate.sh --host
```

缓存镜像路径不是最终 Docker-authoritative release gate 的替代品。

## 打包

```bash
./scripts/package-release.sh
```

脚本写出 `dist/cortex-v${VERSION}-${PLATFORM}.tar.gz` 和对应 `.sha256`。
