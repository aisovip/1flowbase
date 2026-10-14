"use strict";
// Isolated build only; local validation consumes this static UI with Vite preview.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const {spawn, execFileSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../..');
const out = path.join(root, 'tmp/test-governance/agent-logs-otlp/ci-web');
const receipt = {result:'incomplete', source_sha:null, web_tree:null, commands:[], files:[]};
const checkpoint = () => fs.writeFileSync(path.join(out,'receipt.json'), JSON.stringify(receipt,null,2));
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function cpu() {
  return fs.readFileSync('/proc/stat', 'utf8').split('\n')[0].trim().split(/\s+/).slice(1).map(Number);
}
function resources(previous) {
  const current = cpu();
  const delta = current.reduce((n, value, i) => n + value - previous[i], 0);
  const idle = (current[3] - previous[3]) + (current[4] - previous[4]);
  const memory = Object.fromEntries([...fs.readFileSync('/proc/meminfo', 'utf8').matchAll(/^(\w+):\s+(\d+)/gm)].map(m => [m[1], Number(m[2])]));
  const disk = fs.statfsSync(root);
  return { current, sample: {
    cpu_fraction: delta > 0 ? 1 - idle / delta : 0,
    memory_fraction: 1 - memory.MemAvailable / memory.MemTotal,
    disk_fraction: 1 - disk.bavail / disk.blocks
  }};
}

async function command(label, args, env) {
  const log = path.join(out, `${label}.log`);
  const fd = fs.openSync(log, 'w');
  const child = spawn('pnpm', args, { cwd: root, env, detached: true, stdio: ['ignore', fd, fd] });
  let previous = cpu(), stopped = false, killTimer;
  const item = { label, command: ['pnpm', ...args], exit_code: null, signal: null, log: path.basename(log) };
  receipt.commands.push(item);
  checkpoint();
  const monitor = setInterval(() => {
    const observed = resources(previous);
    previous = observed.current;
    fs.appendFileSync(path.join(out, 'resources.jsonl'), JSON.stringify({ label, ...observed.sample }) + '\n');
    if (!stopped && Object.values(observed.sample).some(value => value >= 0.9)) {
      stopped = true;
      item.resource_stop = observed.sample;
      try { process.kill(-child.pid, 'SIGTERM'); } catch { /* child may already have finished */ }
      killTimer = setTimeout(() => { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }, 10000);
    }
  }, 2000);
  try {
    await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => {
        item.exit_code = code;
        item.signal = signal;
        if (code === 0 && !stopped) resolve();
        else reject(new Error(`${label} failed; see ${item.log}`));
      });
    });
  } finally {
    clearInterval(monitor);
    if (stopped) { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }
    clearTimeout(killTimer);
    fs.closeSync(fd);
    checkpoint();
  }
  return log;
}

async function main() {
  assert.equal(process.env.GITHUB_ACTIONS, 'true', 'UI build must run on isolated GitHub runner');
  fs.mkdirSync(out, {recursive:true});
  const git = args => execFileSync('git', args, {cwd:root, encoding:'utf8'}).trim();
  receipt.source_sha = git(['rev-parse','HEAD']);
  assert.match(process.env.EXPECTED_SOURCE_SHA || '', /^[a-f0-9]{40}$/);
  assert.equal(receipt.source_sha, process.env.EXPECTED_SOURCE_SHA);
  receipt.web_tree = git(['rev-parse','HEAD:web']);
  receipt.api_tree = git(['rev-parse','HEAD:api']);
  receipt.pnpm = execFileSync('pnpm',['--version'],{encoding:'utf8'}).trim();
  const actual = os.availableParallelism();
  const allowed = fs.readFileSync('/proc/self/status','utf8').match(/^Cpus_allowed_list:\s*(.+)$/m);
  assert.ok(allowed);
  const ids = allowed[1].trim().split(',').flatMap(part => {
    const [first,last=first] = part.split('-').map(Number);
    return Array.from({length:last-first+1},(_,i)=>first+i);
  });
  const capacity = Math.max(1,Math.floor(Math.min(actual,ids.length)/2));
  const affinity = ids.slice(0,capacity).join(',');
  execFileSync('taskset',['-pc',affinity,String(process.pid)],{stdio:'pipe'});
  receipt.resource_budget = {actual_parallelism:actual,cpu_affinity:affinity};
  const env = {...process.env};
  await command('install',['--dir','web','install','--frozen-lockfile','--reporter=append-only'],env);
  // Existing app/container build path uses Vite; this finite artifact stage
  // moves its cold bundling to CI without expanding task QA to full-repo tsc.
  await command('vite-build',['--dir','web','--filter','@1flowbase/web','exec','vite','build'],env);
  const dist = path.join(out,'dist');
  fs.cpSync(path.join(root,'web/app/dist'),dist,{recursive:true});
  assert.ok(fs.existsSync(path.join(dist,'index.html')));
  function visit(directory, prefix='') {
    for (const entry of fs.readdirSync(directory,{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))) {
      const relative = prefix + entry.name;
      const filename = path.join(directory,entry.name);
      if (entry.isDirectory()) visit(filename,relative+'/');
      else {
        assert.ok(entry.isFile(), 'dist must contain regular files only');
        const bytes = fs.readFileSync(filename);
        receipt.files.push({path:relative,sha256:sha256(bytes),bytes:bytes.length});
      }
    }
  }
  visit(dist);
  receipt.files.sort((a,b)=>a.path.localeCompare(b.path));
  receipt.dist_digest = sha256(Buffer.from(JSON.stringify(receipt.files)));
  receipt.result='pass';
  checkpoint();
  process.stdout.write(JSON.stringify({result:receipt.result,source_sha:receipt.source_sha,web_tree:receipt.web_tree,files:receipt.files.length,dist_digest:receipt.dist_digest})+'\n');
}
main().catch(error => {
  receipt.result='fail';
  if(fs.existsSync(out)) checkpoint();
  process.stderr.write(error.message+'\n');
  process.exitCode=1;
});
