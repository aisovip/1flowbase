import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const baseline = '6a0530638c5839ed19893a16fca3a4b890af8d4e';
const output = path.resolve('tmp/test-governance/legacy-crypto-fixtures');
const source = path.join(output, 'old-source');
const producer = path.join(output, 'producer');
const artifact = path.join(output, 'artifact');
const mode = process.argv[2];
if (process.env.GITHUB_ACTIONS !== 'true') throw new Error('legacy fixture production is CI-only');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
}
function packages(lock) {
  return lock.split('[[package]]').slice(1).map((block) => {
    const field = (name) => block.match(new RegExp(`^${name} = "([^"]+)"`, 'm'))?.[1];
    return { name: field('name'), version: field('version'), source: field('source'), checksum: field('checksum') };
  });
}
const includedSources = [
  'api/crates/control-plane/src/auth/mod.rs',
  'api/crates/control-plane/src/system_backup/envelope.rs',
  'api/crates/control-plane/src/system_backup/service.rs',
  'api/crates/storage/durable/postgres/src/secret_crypto/mod.rs',
];
if (mode === 'prepare') {
  mkdirSync(source, { recursive: true });
  mkdirSync(path.join(producer, 'src'), { recursive: true });
  mkdirSync(artifact, { recursive: true });
  const archive = path.join(output, 'old-source.tar');
  run('git', ['archive', '--format=tar', `--output=${archive}`, baseline, 'api']);
  run('tar', ['-xf', archive, '-C', source]);
  const quote = (value) => JSON.stringify(value);
  const deps = [
    'anyhow = "=1.0.102"', 'argon2 = "=0.5.3"', 'base64 = "=0.22.1"',
    'chacha20poly1305 = "=0.10.1"', 'hmac = "=0.12.1"',
    'ed25519-dalek = { version = "=2.2.0", features = ["pem"] }',
    'rand_core = { version = "=0.6.4", features = ["std"] }',
    'sha2 = "=0.10.9"', 'serde_json = { version = "=1.0.149", features = ["preserve_order"] }',
    'thiserror = "=2.0.18"', 'time = { version = "=0.3.47", features = ["serde", "formatting", "parsing"] }',
    'tokio = { version = "=1.51.1", features = ["full"] }',
    `domain = { path = ${quote(path.join(source, 'api/crates/domain'))} }`,
    `control-plane-contracts = { path = ${quote(path.join(source, 'api/crates/control-plane-contracts'))} }`,
  ];
  writeFileSync(path.join(producer, 'Cargo.toml'), `[package]\nname = "legacy-crypto-fixture-producer"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n\n[dependencies]\n${deps.join('\n')}\n`);
  copyFileSync(path.join(source, 'api/Cargo.lock'), path.join(producer, 'Cargo.lock'));
  copyFileSync(path.join(source, includedSources[1]), path.join(producer, 'src/envelope.rs'));
  copyFileSync(path.join(source, includedSources[3]), path.join(producer, 'src/secret.rs'));
  const auth = readFileSync(path.join(source, includedSources[0]), 'utf8');
  const authFunction = auth.slice(auth.indexOf('fn hash_password(password: &str)'));
  if (!authFunction.startsWith('fn hash_password(') || !authFunction.includes('SaltString::generate')) throw new Error('old auth source boundary changed');
  writeFileSync(path.join(producer, 'src/password.rs'), `use anyhow::Result;\nuse argon2::{password_hash::SaltString, Argon2, PasswordHasher};\nuse rand_core::OsRng;\n${authFunction}\npub fn generate(password: &str) -> Result<String> { hash_password(password) }\n`);
  const service = readFileSync(path.join(source, includedSources[2]), 'utf8');
  const start = service.indexOf('pub(crate) struct PasswordProtection');
  const end = service.indexOf('fn checked_key_material(', start);
  if (start < 0 || end < 0) throw new Error('old password backup source boundary changed');
  writeFileSync(path.join(producer, 'src/protection.rs'), `use argon2::Argon2;\nuse base64::{engine::general_purpose::STANDARD, Engine as _};\nuse chacha20poly1305::{aead::{Aead, Payload}, KeyInit, XChaCha20Poly1305, XNonce};\nuse domain::KeyFingerprint;\nuse rand_core::{OsRng, RngCore};\nuse sha2::{Digest, Sha256};\nuse crate::ports::BackupKeyMaterial;\n${service.slice(start, end)}\n`);
  copyFileSync('scripts/node/legacy-crypto-fixtures/main.rs', path.join(producer, 'src/main.rs'));
} else if (mode === 'verify-lock' || mode === 'provenance') {
  const oldLock = readFileSync(path.join(source, 'api/Cargo.lock'));
  const producerLock = readFileSync(path.join(producer, 'Cargo.lock'));
  const oldPackages = packages(oldLock.toString());
  const resolved = packages(producerLock.toString()).filter((entry) => entry.source);
  for (const entry of resolved) {
    if (!oldPackages.some((old) => old.name === entry.name && old.version === entry.version && old.source === entry.source && old.checksum === entry.checksum)) {
      throw new Error(`producer resolved a dependency absent from immutable old lock: ${JSON.stringify(entry)}`);
    }
  }
  if (mode === 'provenance') {
    const fixtures = readFileSync(path.join(artifact, 'fixtures.json'));
    const candidate = spawnSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' });
    if (candidate.status !== 0) throw new Error('cannot resolve producer source');
    writeFileSync(path.join(artifact, 'provenance.json'), `${JSON.stringify({
      schema_version: 1, baseline, producer_commit: candidate.stdout.trim(),
      workflow_run: process.env.GITHUB_RUN_ID, workflow_attempt: process.env.GITHUB_RUN_ATTEMPT,
      workflow_url: `https://github.com/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`,
      fixture_sha256: sha256(fixtures), baseline_lock_sha256: sha256(oldLock), producer_lock_sha256: sha256(producerLock),
      source_sha256: Object.fromEntries(includedSources.map((name) => [name, sha256(readFileSync(path.join(source, name)))])),
      resolved_external_packages: resolved,
    }, null, 2)}\n`);
    copyFileSync(path.join(source, 'api/Cargo.lock'), path.join(artifact, 'baseline.Cargo.lock'));
    copyFileSync(path.join(producer, 'Cargo.lock'), path.join(artifact, 'producer.Cargo.lock'));
  }
} else throw new Error('expected prepare, verify-lock, or provenance');
