FROM rust:latest AS dev

RUN rustup component add rustfmt clippy

RUN groupadd -g 1000 dev && useradd -m -u 1000 -g dev dev

# Pre-create cargo directories so volumes mount with correct ownership
RUN mkdir -p /home/dev/.cargo/registry /home/dev/.cargo/git \
    && chown -R dev:dev /home/dev/.cargo

ENV CARGO_HOME=/home/dev/.cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

USER dev

WORKDIR /workspace
