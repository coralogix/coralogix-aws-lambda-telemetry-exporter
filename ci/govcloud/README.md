# GovCloud build inputs

The GovCloud layers must link a NIST-validated crypto module. Mainline resolves
`aws-lc-rs 1.18` / `aws-lc-fips-sys 0.14`, which builds the AWS-LC FIPS 4.x module:
submitted to NIST, not yet certified. The GovCloud build instead uses the certified
AWS-LC FIPS 3.x module (certificates #5314 and #5298), through two files used only
for that build:

- `Cargo.lock`: the mainline lockfile with the crypto stack held back.
- `config.toml`: patches `rustls-webpki` to `vendor/rustls-webpki`, a copy of 0.103.16
  (which carries the eMudhra distrust fix) with ML-DSA reverted, the one change that
  forces `aws-lc-rs 1.18`. See `vendor/rustls-webpki/PATCH.md`. The same copy is used
  by `coralogix-aws-shipper`.

To build:

```bash
mkdir -p .cargo
cp ci/govcloud/config.toml .cargo/config.toml
cp ci/govcloud/Cargo.lock Cargo.lock
cargo build --release --locked --no-default-features --features fips
```

## Differences from mainline

| Crate | Mainline | GovCloud | Why |
|---|---|---|---|
| `aws-lc-fips-sys` | 0.14.3 (FIPS 4.x) | 0.13.14 (AWS-LC FIPS 3.3.0) | the certified module |
| `aws-lc-rs` | 1.18.2 | 1.17.0 | the last release on the 3.x module |
| `aws-lc-sys` | 0.46.0 | 0.41.0 | follows `aws-lc-rs` |
| `rustls` | 0.23.46 | 0.23.36 | newer releases require `aws-lc-rs` 1.18 |
| `rustls-webpki` | 0.103.16 | 0.103.16, vendored | ML-DSA reverted, see above |
| `bindgen`, `prettyplease`, `shlex` | | | build-time only, follow `aws-lc-*-sys` |

Everything else matches mainline.

## Accepted risks

Holding `rustls` at 0.23.36 keeps four advisories that mainline has fixed. Two of them
apply to this build and are **accepted, not fixed**. `cargo deny` passes only because
`deny.toml` lists them:

- **RUSTSEC-2026-0285:** rustls accepts some TLS 1.3 handshake messages at the wrong
  encryption level. The handshake stays authenticated; a peer can only send in plaintext
  what should have been encrypted.
- **RUSTSEC-2026-0349:** with `prefer-post-quantum` on, an unauthenticated ServerHello can
  push later connections to the same server onto classical key exchange. Classical TLS
  protection is unaffected; only the post-quantum protection is lost.

The other two (RUSTSEC-2026-0346 and RUSTSEC-2026-0348) don't apply; see `deny.toml`.
All four are fixed by `rustls` 0.23.46, which needs the uncertified FIPS 4.x module, so
they go away when this directory does.

## Updating

Regenerate from the current mainline lockfile:

```bash
mkdir -p .cargo && cp ci/govcloud/config.toml .cargo/config.toml
cargo update -p rustls --precise 0.23.36
cargo update -p aws-lc-fips-sys --precise 0.13.14
cargo tree --locked --no-default-features --features fips -i aws-lc-fips-sys   # must show 0.13.14
cp Cargo.lock ci/govcloud/Cargo.lock && git checkout Cargo.lock && rm -r .cargo
```

Remove the v3 workaround (this directory and `vendor/`) once v4 is validated, its
documented requirements fit our build and runtime environments, and the replacement
passes our release checks.
