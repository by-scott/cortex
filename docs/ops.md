# Operations

## Available CLI

```bash
cortex version
cortex status
cortex release-plan
cortex help
```

`status` reports the 1.5 rewrite surface: strict gate, multi-user ownership,
journal recovery, RAG, and plugin authorization.

`release-plan` prints the required release order:

1. backup
2. migrate
3. install
4. smoke-test
5. package
6. publish

## Gate

The release gate is:

```bash
./scripts/gate.sh --docker
```

When Docker Hub is unreachable, the same checks can run inside the already
built gate image:

```bash
docker run --rm -e CORTEX_GATE_IN_DOCKER=1 \
  -v cortex-gate-cargo:/home/dev/.cargo \
  -v "$PWD":/workspace -w /workspace \
  cortex-gate:latest ./scripts/gate.sh --host
```

The cached-image path is not a substitute for the final Docker-authoritative
release gate.

## Packaging

```bash
./scripts/package-release.sh
```

This writes `dist/cortex-v${VERSION}-${PLATFORM}.tar.gz` and the matching
`.sha256` file.
