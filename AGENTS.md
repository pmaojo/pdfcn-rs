# pdfcn-rs — agent instructions

HAML-like templates + shadcn-style components + Tailwind-style utility
classes, compiled to PDF bytes in memory. Pure Rust: no headless browser,
no process spawning, safe to run inside a Vercel Function. Rendering is
done through `printpdf`'s pure-Rust HTML/CSS layout engine (`azul-layout`),
never a browser.

## Build, test, lint

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Two opt-in Cargo features exist beyond the default build — always test
each of them explicitly too, since CI does, and a change gated behind a
feature is only proven correct if it's actually compiled:

```sh
cargo test --workspace --features pdfcn-core/vector,pdfcn-components/vector,pdfcn-cli/vector
cargo test --workspace --features pdfcn-core/factur-x,pdfcn-cli/factur-x
```

The serverless binary (`pdfcn-vercel`, the root package under `api/`) has
a hard size gate in CI. Never build it with `--workspace` (resolver="2"
would unify features across packages and defeat the gate):

```sh
cargo build --release -p pdfcn-vercel --bin generate-pdf
cargo tree --edges features -p pdfcn-vercel   # confirm resvg/lopdf absent
```

## Workspace layout

| Crate | Responsibility |
|---|---|
| `pdfcn-parser` | Indentation-based HAML-like lexer/parser (`winnow`) → AST |
| `pdfcn-template` | `{{ interpolation }}`, `- for`, `- if`, `- include` (`minijinja`) |
| `pdfcn-components` | `%InvoiceTable`, `%Badge`, `%Card`, `%LineChart`, `%Barcode`, ... registry |
| `pdfcn-styles` | Zero-Node Tailwind-style utility scanner + print-safe CSS (`lightningcss`) |
| `pdfcn-core` | Orchestrates the pipeline; HTML → PDF; the two opt-in Cargo features live here |
| `pdfcn-cli` | `pdfcn new / add / build / dev` |
| `api/generate-pdf.rs`, `api/generate-pdf-batch.rs` | Vercel Function handlers |
| `pdfcn-node` | `napi-rs` bindings for Next.js |
| `.claude-plugin/`, `skills/` | An in-repo Claude Code plugin teaching agents the template syntax and CLI (load with `claude --plugin-dir .`) |

## Conventions that matter here

- **Every new capability that isn't tiny goes behind an opt-in Cargo
  feature**, off by default (`vector` for Charts v2/`%Barcode`/`%Vector`,
  `factur-x` for invoice embedding). Verify with `cargo tree --edges
  features -p pdfcn-vercel` that the default build never sees it.
- **Nothing panics on bad input.** A malformed component, an unencodable
  barcode value, a corrupt input PDF — all degrade to `None`/an explicit
  error/an inline marker, never a crash. Grep for `unwrap()`/`expect()`
  outside `#[cfg(test)]` before adding either.
- **Golden-PDF snapshots** (`pdfcn-core/tests/golden.rs`) render every
  template in `examples/` and diff against a stable digest. A layout
  change updates a snapshot deliberately, not by surprise — regenerate
  and say so in the commit, don't silently accept a diff.
- **Gherkin coverage** lives in `features/*.feature`, each scenario tagged
  `@covers(path/to/file.rs)` naming exactly the file it exercises — never
  a wildcard. Add scenarios alongside new modules, matching the existing
  files' style.
- **Docs before code for anything non-obvious.** Non-trivial capabilities
  (the vector substrate, Factur-X) got a short spike doc under
  `docs/spikes/` recording what was verified against the real upstream
  source (printpdf, lopdf) before writing the implementation — not
  assumptions. Do the same for the next one; check that directory first,
  a relevant spike may already answer your question.
- **Verify library behavior against its actual source**, not memory —
  this codebase has repeatedly found real gaps that way (printpdf's own
  header/footer rendering is wired but a no-op in 0.12.6; it has zero
  embedded-file support). `printpdf`, `lopdf`, `resvg` sources are in
  `~/.cargo/registry/src/`.
- **Never commit a git amend/force-push** without being asked; this repo
  develops on feature branches merged via PR.
