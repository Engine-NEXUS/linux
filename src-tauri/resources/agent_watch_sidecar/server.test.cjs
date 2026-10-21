const test = require('node:test');
const assert = require('node:assert/strict');
const net = require('node:net');
const path = require('node:path');
const { spawn } = require('node:child_process');

async function freePort() {
  const listener = net.createServer();
  await new Promise((resolve) => listener.listen(0, '127.0.0.1', resolve));
  const port = listener.address().port;
  await new Promise((resolve) => listener.close(resolve));
  return port;
}

test('local watcher rejects unauthenticated control and blank tasks', async () => {
  const port = await freePort();
  const token = 'a'.repeat(64);
  const child = spawn(process.execPath, [path.join(__dirname, 'server.cjs')], {
    cwd: __dirname,
    env: { ...process.env, AGENT_WATCH_PORT: String(port), AGENT_WATCH_TOKEN: token, NEXUS_PARENT_PID: String(process.pid) },
    stdio: ['pipe', 'ignore', 'ignore'],
  });
  try {
    const url = `http://127.0.0.1:${port}`;
    let ready = false;
    for (let i = 0; i < 40; i++) {
      try {
        const response = await fetch(`${url}/health`, { headers: { 'x-nexus-token': token } });
        if (response.ok) { ready = true; break; }
      } catch { /* starting */ }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    assert.equal(ready, true, 'sidecar should start');
    assert.equal((await fetch(`${url}/health`)).status, 403);
    assert.equal((await fetch(`${url}/health`, { headers: { 'x-nexus-token': 'b'.repeat(64) } })).status, 403);
    const blank = await fetch(`${url}/watch`, {
      method: 'POST',
      headers: { 'x-nexus-token': token, 'content-type': 'application/json' },
      body: JSON.stringify({ type: 'claude' }),
    });
    assert.equal(blank.status, 400);
    const watches = await fetch(`${url}/watch`, { headers: { 'x-nexus-token': token } });
    assert.deepEqual((await watches.json()).watches, []);
    const exited = new Promise((resolve) => child.once('exit', resolve));
    child.stdin.end();
    const result = await Promise.race([
      exited,
      new Promise((_, reject) => setTimeout(() => reject(new Error('sidecar survived parent pipe close')), 2000)),
    ]);
    assert.equal(result, 0);
  } finally {
    child.kill();
  }
});
