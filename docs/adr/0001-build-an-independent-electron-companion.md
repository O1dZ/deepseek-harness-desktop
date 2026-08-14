---
status: superseded by ADR-0002
---

# Build an independent Electron companion

DeepSeek Harness Desktop will be a separate community project that embeds the official `@deepseek-ai/dsh` runtime instead of forking DeepSeek Harness or reimplementing its client protocol. Electron is accepted because it can package and supervise the Node.js runtime directly; this trades a larger application footprint for a self-contained Windows installation, a smaller integration surface, and easier compatibility with upstream Harness releases.
