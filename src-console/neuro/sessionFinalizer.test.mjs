import assert from 'node:assert/strict';
import test from 'node:test';
import { SessionFinalizer } from './sessionFinalizer.ts';

test('save retry preserves payload and does not stop or snapshot again', async () => {
  const finalizer = new SessionFinalizer();
  const calls = [];
  const payload = { id: 'one-session', end: 123, samples: 500 };
  let fails = true;
  const steps = {
    stopStream: async () => { calls.push('stream'); },
    stopRecording: async () => { calls.push('recording'); },
    snapshot: () => { calls.push('snapshot'); return payload; },
    save: async (value) => {
      assert.equal(value, payload);
      calls.push('save');
      if (fails) throw new Error('disk full');
    },
  };
  await assert.rejects(finalizer.run(steps), /disk full/);
  fails = false;
  await finalizer.run(steps);
  await finalizer.run(steps);
  assert.deepEqual(calls, ['stream', 'recording', 'snapshot', 'save', 'save']);
});

test('failed recorder flush is retried before any save', async () => {
  const finalizer = new SessionFinalizer();
  let stops = 0, flushes = 0, saves = 0;
  const steps = {
    stopStream: async () => { stops++; },
    stopRecording: async () => { if (++flushes === 1) throw new Error('flush'); },
    snapshot: () => ({}),
    save: async () => { saves++; },
  };
  await assert.rejects(finalizer.run(steps), /flush/);
  assert.equal(saves, 0);
  await finalizer.run(steps);
  assert.deepEqual([stops, flushes, saves], [1, 2, 1]);
});

test('concurrent stop requests share one finalization', async () => {
  const finalizer = new SessionFinalizer();
  let release;
  let saves = 0;
  const steps = {
    stopStream: () => new Promise((resolve) => { release = resolve; }),
    stopRecording: async () => {},
    snapshot: () => ({}),
    save: async () => { saves++; },
  };
  const first = finalizer.run(steps);
  const second = finalizer.run(steps);
  assert.equal(first, second);
  release();
  await first;
  assert.equal(saves, 1);
});

test('stream failure cannot flush or save prematurely', async () => {
  const finalizer = new SessionFinalizer();
  await assert.rejects(finalizer.run({
    stopStream: async () => { throw new Error('stream'); },
    stopRecording: async () => { assert.fail('must not flush'); },
    snapshot: () => assert.fail('must not snapshot'),
    save: async () => { assert.fail('must not save'); },
  }), /stream/);
});
