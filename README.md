# srvcs-lcm

The least-common-multiple orchestrator of the srvcs.cloud distributed standard
library.

Its single concern: **number theory: least common multiple.** It owns the
*control flow* — composing three primitives — but does no arithmetic of its own.
It asks [`srvcs-gcd`](https://github.com/srvcs/gcd) for the greatest common
divisor, then [`srvcs-divide`](https://github.com/srvcs/divide) and
[`srvcs-multiply`](https://github.com/srvcs/multiply) to assemble the result.

```
lcm(a, b):
    g = gcd(a, b)
    if g == 0:
        return 0          # gcd(0, 0) == 0
    q = divide(a, g)
    return multiply(q, b) # (a / gcd(a, b)) * b
```

`lcm(0, 0) == 0` falls out naturally: `gcd(0, 0) == 0` short-circuits the result
to `0` without calling `srvcs-divide` or `srvcs-multiply`.

Validation is not handled here. This service never calls `srvcs-isnumber`
directly; instead its dependencies validate their own operands, and any `422`
they raise is forwarded verbatim.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Service identity, concern, and dependency list |
| `POST` | `/` | Compute `lcm(a, b)` |
| `GET` | `/healthz` `/readyz` `/metrics` `/openapi.json` | srvcs service standard surface |

```sh
curl -s -X POST localhost:8080/ -H 'content-type: application/json' -d '{"a": 4, "b": 6}'
# {"a":4,"b":6,"result":12}
```

Responses:

- `200 {"a": a, "b": b, "result": n}` — evaluated; `result` is an integer.
- `422` — a dependency rejected the input, forwarded verbatim.
- `500` — a reachable dependency returned a `200` without an integer `result`
  (a contract violation).
- `503` — a dependency is unavailable.

## Dependencies

- [`srvcs-gcd`](https://github.com/srvcs/gcd)
- [`srvcs-divide`](https://github.com/srvcs/divide)
- [`srvcs-multiply`](https://github.com/srvcs/multiply)

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SRVCS_BIND_ADDR` | `0.0.0.0:8080` | Bind address |
| `SRVCS_GCD_URL` | `http://127.0.0.1:8090` | Base URL of `srvcs-gcd` |
| `SRVCS_DIVIDE_URL` | `http://127.0.0.1:8091` | Base URL of `srvcs-divide` |
| `SRVCS_MULTIPLY_URL` | `http://127.0.0.1:8092` | Base URL of `srvcs-multiply` |
| `SRVCS_ENV` | `development` | Environment label for logs |
| `RUST_LOG` | `info,tower_http=info` | Tracing filter |

## Local checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Orchestration tests stand up *computing* mock `srvcs-gcd`, `srvcs-divide` and
`srvcs-multiply` services in-process — they read the request body and return the
real `gcd(a, b)` / `a / b` / `a * b`, so the composition is genuinely exercised
against the asserted cases. See
[`srvcs/platform`](https://github.com/srvcs/platform) for the shared standard.

> Note: the `cargoHash` in `flake.nix` is inherited from the template and must be
> refreshed with a `nix build` before the Nix gates pass.
