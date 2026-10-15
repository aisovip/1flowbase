# Historical crypto fixtures

This manual, read-only workflow produces public test data with immutable source
`6a0530638c5839ed19893a16fca3a4b890af8d4e` and its original Cargo lock. It is not a
candidate compatibility test or a product dependency override. Run it online:

```sh
gh workflow run legacy-crypto-fixtures.yml --ref BRANCH -f source_ref=EXACT_PRODUCER_COMMIT
```

The script exports the old `api/` tree using `git archive` into ignored
`tmp/test-governance/legacy-crypto-fixtures/old-source`. An isolated producer Cargo
workspace path-depends on that tree's domain/contracts, includes its unmodified
secret and backup envelope modules, and extracts the exact original password
hash and password-protection functions. Their new preludes only provide the
original imports/ports; the extracted function bodies are not rewritten.

The copied old lock seeds dependency resolution. Before compiling/running, every
resolved external package's name, version, source and checksum must match a
package in that immutable lock; the direct crypto versions are exact-pinned.
The producer never links the candidate crypto graph. No product manifest or
product lock is modified. Source archive exports are not Git checkouts/worktrees.

The artifact contains `fixtures.json`, immutable-source hashes, workflow URL,
producer commit, fixture hash and both Cargo locks. Candidate tests must consume
the downloaded static bytes and retain provenance, rather than create expected
ciphertext, hashes or signatures with the candidate. Random salts/nonces make
separate producer runs differ; checked-in artifacts remain fixed to their run.
All passwords, master keys, seeds and payloads are synthetic public test data.
