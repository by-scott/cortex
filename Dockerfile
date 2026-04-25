FROM rust@sha256:e4f09e8fe5a2366e7d3dc35e08bd25821151e3ed8fdbd3a6a16b51555f0c551d AS dev

RUN apt-get update \
    && apt-get install -y --no-install-recommends ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

RUN groupadd -g 1000 dev && useradd -m -u 1000 -g dev dev

# Pre-create cargo directories so volumes mount with correct ownership
RUN mkdir -p /home/dev/.cargo/registry /home/dev/.cargo/git \
    && chown -R dev:dev /home/dev/.cargo

ENV CARGO_HOME=/home/dev/.cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

USER dev

WORKDIR /workspace
