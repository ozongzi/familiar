# familiar-sandbox

Multi-language coding sandbox image used by the familiar backend.

## Languages included

- **Rust** (stable, clippy, rustfmt, cargo)
- **Node.js** LTS + npm + bun
- **Python 3** + pip + uv
- **Go** (latest stable)
- **Java 21** (JDK) + Maven + Gradle

## CLI tools

git, curl, wget, jq, ripgrep, fd-find, tree, make, gcc

## Build

```bash
docker build -t familiar-sandbox:latest docker/sandbox/
```

## Update image name in sandbox.rs

The image name is hardcoded as `familiar-sandbox:latest` in `backend/src/sandbox.rs`.
