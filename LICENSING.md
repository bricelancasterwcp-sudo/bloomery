# Licensing

bloomery is **dual-licensed**. The same code is available two ways, and you
choose which one you are using:

1. **GNU Affero General Public License v3.0 (AGPL-3.0-only)** — free of
   charge, for everyone, including commercial use, on the condition that you
   honour the AGPL's terms. Full text: [`LICENSE`](LICENSE).
2. **A commercial license** — a separate, negotiated grant that removes the
   AGPL's source-disclosure obligations. See [Commercial licensing](#commercial-licensing).

There is no feature difference between the two. The code is identical. What
differs is what you owe in return.

## What is covered by what

| tree | license | why |
|---|---|---|
| `crates/` (Rust) | AGPL-3.0-only | the daemon, core, substrate, bench |
| `tools/` (Python) | AGPL-3.0-only | the flywheel factory, batteries, evidence tooling |
| `docs/` | CC BY 4.0 ([`docs/LICENSE`](docs/LICENSE)) | specs, plans, evidence, findings |
| `Cargo.lock`, config samples | AGPL-3.0-only | part of the build |

The `docs/` tree is deliberately **not** copyleft. The measurement record —
pre-registrations, batteries, evidence documents, the carried-debt ledger — is
meant to be quoted, cited, reproduced and argued with, including by people who
will never touch the code. Attribution is the only condition.

Third-party dependencies keep their own licenses. Every current dependency is
permissive (MIT / Apache-2.0; `self_cell` is used under its Apache-2.0 arm),
and llama.cpp — reached through `llama-cpp-2` — is MIT. Nothing in the
dependency graph imposes copyleft on you; the AGPL here is a deliberate choice
by the copyright holder, not an inherited obligation.

## What the AGPL means here, in plain terms

This is a non-binding summary. [`LICENSE`](LICENSE) is the authoritative text
and governs if the two disagree.

**You may**, at no cost: run bloomery for any purpose including commercial
purposes, study it, modify it, and distribute it or your modifications.

**You must**, if you do: keep it under AGPL-3.0, preserve the copyright and
license notices, and make the corresponding source available to the people you
give it to.

**The clause that matters for a daemon.** bloomery is a network service — it
exposes an HTTP API and an OpenAI-compatible shim. AGPL section 13 says that if
you modify bloomery and let users interact with it **over a network**, those
users must be offered the source of your modified version, even though you never
"distributed" a binary to them. Ordinary GPL would not reach that case. This is
the specific reason bloomery is AGPL rather than GPL.

**Running an unmodified bloomery internally** triggers no disclosure obligation.
Using bloomery's HTTP API from your own separate program is not, by itself,
modifying bloomery.

Where the boundary falls for a given deployment is a legal question about your
facts, not a technical one. If the answer matters to you, get advice or take a
commercial license.

## Commercial licensing

A commercial license is the right choice if any of these are true:

- You want to embed or ship bloomery inside a product without releasing that
  product's source.
- You want to offer a modified bloomery as a network service without meeting
  section 13.
- Your organisation's policy prohibits AGPL-licensed software regardless of
  how it is used.
- You want warranty, indemnity, support, or a defined security-response
  commitment. The AGPL grant carries none of these — it is offered **as is**,
  with no warranty (see LICENSE sections 15–16).

**To enquire:** open an issue titled `commercial licensing` on
<https://github.com/bricelancasterwcp-sudo/bloomery>, or email
`bricelancaster.wcp@gmail.com` with your intended use, deployment shape
(internal / embedded / hosted service), and rough scale. Terms are negotiated
per deal; there is no published price list.

Buying a commercial license does not remove the AGPL option for anyone else,
and does not make any released version proprietary. It grants *you*
alternative terms for *your* use.

## Contributions

bloomery can only be dual-licensed while a single party holds the right to
license all of it. That means contributions need an explicit grant before they
can be merged — see [`CLA.md`](CLA.md). It is short, you keep your copyright,
and it is a one-time thing per contributor.

If you would rather not sign it, that is a legitimate choice: open an issue
describing the change instead of a pull request, and it can be implemented
independently.

## Sibling repositories

bloomery's measurements depend on sibling projects — notably
[assay](https://github.com/bricelancasterwcp-sudo/assay), which bloomery
invokes as a subprocess for boot-time capability profiling, and
[gguf-geometry](https://github.com/bricelancasterwcp-sudo/gguf-geometry),
whose contract vectors are vendored into the test suite.

**Those repositories carry their own licenses and are not covered by this
document.** Invoking assay as a separate process is not linking, and vendored
gguf-geometry test vectors are data. If you are evaluating bloomery for
commercial use, raise the sibling repositories in the same conversation so the
whole surface is settled at once.

## Why this structure

Stated plainly, because the alternative is guessing:

- **AGPL rather than a non-commercial license**, because bloomery needs users
  more than it needs a moat. A "free for individuals, paid for companies"
  licence would be source-available, not open source, and would suppress
  exactly the adoption that produces field evidence — which is what this
  project runs on.
- **AGPL rather than MIT**, because the copyleft is the only thing that makes
  a commercial license worth buying, and because a permissive grant is
  irreversible: released versions can never be pulled back.
- **AGPL-3.0-only rather than -or-later**, so that a future FSF licence
  revision cannot change the terms of the offer without the copyright holder
  deciding to adopt it.

## Not legal advice

This file is a description of an intent, written by the project, for
engineers. It is not legal advice and creates no obligations on its own. The
operative documents are [`LICENSE`](LICENSE), [`docs/LICENSE`](docs/LICENSE),
[`CLA.md`](CLA.md), and any commercial agreement actually signed.

Copyright © 2026 Brice Lancaster.
