# familiar-sandbox — multi-language coding sandbox image
#
# Languages / runtimes included:
#   Rust (stable + clippy + rustfmt + cargo)
#   Node.js LTS + npm + bun
#   Python 3 + pip + uv
#   Go (latest stable)
#   Java 21 (JDK) + Maven + Gradle
#
# CLI tools:
#   git, curl, wget, jq, ripgrep, fd-find, tree, unzip, make, gcc

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8

# ── System packages ────────────────────────────────────────────────────────────
RUN apt-get update && apt-get install -y --no-install-recommends \
    # build essentials
    build-essential gcc g++ make cmake pkg-config \
    openssh-client \
    # shell / utils
    curl wget git jq unzip zip tree \
    ca-certificates gnupg lsb-release \
    # search tools
    ripgrep fd-find \
    # Python
    python3 python3-pip python3-venv python3-dev \
    # Java 21
    openjdk-21-jdk maven \
    && rm -rf /var/lib/apt/lists/*

# ── Rust ───────────────────────────────────────────────────────────────────────
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal \
    && rustup component add clippy rustfmt \
    && chmod -R a+w /usr/local/cargo /usr/local/rustup

# ── Node.js LTS (via NodeSource) ───────────────────────────────────────────────
RUN curl -fsSL https://deb.nodesource.com/setup_lts.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# ── Bun ───────────────────────────────────────────────────────────────────────
RUN curl -fsSL https://bun.sh/install | bash \
    && ln -s /root/.bun/bin/bun /usr/local/bin/bun

# ── uv (fast Python package manager) ─────────────────────────────────────────
RUN curl -LsSf https://astral.sh/uv/install.sh | sh \
    && ln -s /root/.local/bin/uv /usr/local/bin/uv

# ── Go ────────────────────────────────────────────────────────────────────────
ARG GO_VERSION=1.23.4
RUN curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz" \
    | tar -C /usr/local -xz
ENV PATH=/usr/local/go/bin:$PATH
ENV GOPATH=/root/go
ENV PATH=$GOPATH/bin:$PATH

# ── Gradle ────────────────────────────────────────────────────────────────────
ARG GRADLE_VERSION=8.10
RUN curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" \
    -o /tmp/gradle.zip \
    && unzip -q /tmp/gradle.zip -d /opt \
    && ln -s /opt/gradle-${GRADLE_VERSION}/bin/gradle /usr/local/bin/gradle \
    && rm /tmp/gradle.zip

WORKDIR /workspace

CMD ["tail", "-f", "/dev/null"]
