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

`vector` (Charts v2/`%Barcode`/`%Vector`) and `factur-x` (invoice
embedding) are on by default -- but still real Cargo features (see
"Cargo features" below), so also test with them explicitly off, since a
change gated behind `#[cfg(feature = "...")]` is only proven correct in
both states:

```sh
cargo test -p pdfcn-core --no-default-features   # per-package, not --workspace:
cargo test -p pdfcn-cli --no-default-features    # pdfcn-vercel/pdfcn-node hard-require
cargo test --workspace                           # both features regardless of the flag,
                                                  # so a workspace-wide --no-default-features
                                                  # wouldn't actually disable them there
```

The serverless binary (`pdfcn-vercel`, the root package under `api/`) has
a hard size gate in CI. Never build it with `--workspace` (resolver="2"
would unify features across packages and defeat the gate):

```sh
cargo build --release -p pdfcn-vercel --bin generate-pdf
cargo tree --edges features -p pdfcn-vercel   # resvg/lopdf are expected now
```

## Workspace layout

| Crate | Responsibility |
|---|---|
| `pdfcn-parser` | Indentation-based HAML-like lexer/parser (`winnow`) → AST |
| `pdfcn-template` | `{{ interpolation }}`, `- for`, `- if`, `- include` (`minijinja`) |
| `pdfcn-components` | `%InvoiceTable`, `%Badge`, `%Card`, `%LineChart`, `%Barcode`, ... registry |
| `pdfcn-styles` | Zero-Node Tailwind-style utility scanner + print-safe CSS (`lightningcss`) |
| `pdfcn-core` | Orchestrates the pipeline; HTML → PDF; the two Cargo features (`vector`, `factur-x`, both on by default) live here |
| `pdfcn-cli` | `pdfcn new / add / build / dev` |
| `api/generate-pdf.rs`, `api/generate-pdf-batch.rs` | Vercel Function handlers |
| `pdfcn-node` | `napi-rs` bindings for Next.js |
| `.claude-plugin/`, `skills/` | An in-repo Claude Code plugin teaching agents the template syntax and CLI (load with `claude --plugin-dir .`) |

## Conventions that matter here

- **Every new capability that isn't tiny goes behind a named Cargo
  feature** (`vector` for Charts v2/`%Barcode`/`%Vector`, `factur-x` for
  invoice embedding are the existing examples) so it can be compiled out
  with `--no-default-features` -- that doesn't mean off by default,
  though: both of those ship on by default because they're each only a
  couple MB. Whether a *new* one defaults on or off is a real judgment
  call against the size gate below, not an automatic "off". Verify with
  `cargo tree --edges features -p pdfcn-vercel` either way, so the actual
  dependency graph of the default build is never a surprise.
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
