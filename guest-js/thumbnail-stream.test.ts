import assert from 'node:assert/strict';
import { test } from 'node:test';

import { streamThumbnails, type StreamedThumbnailInfo } from './index';

type RawMessage = { index: number; message: unknown };

type Call = { cmd: string; args: Record<string, unknown> };

const CHANNEL_ID = 7;

/**
 * Stands in for the Tauri webview IPC. `stream_thumbnails` settles with
 * `scheduled` and `cancel_thumbnail_stream` with `cancelled`; the test pushes
 * channel messages, in any order, through `deliver`.
 */
function mockIpc(
   scheduled: Promise<null> = Promise.resolve(null),
   cancelled: Promise<null> = Promise.resolve(null),
): { calls: Call[]; callbackCount(): number; deliver(index: number, message: unknown): void } {
   const callbacks = new Map<number, (raw: RawMessage) => void>();
   let nextId = CHANNEL_ID;
   const calls: Call[] = [];
   (globalThis as Record<string, unknown>).window = {
      __TAURI_INTERNALS__: {
         transformCallback(fn: (raw: RawMessage) => void): number {
            const id = nextId++;
            callbacks.set(id, fn);
            return id;
         },
         unregisterCallback(id: number): void {
            callbacks.delete(id);
         },
         invoke: (cmd: string, args: Record<string, unknown>): Promise<null> => {
            calls.push({ cmd, args });
            return cmd === 'plugin:media-parser|cancel_thumbnail_stream' ? cancelled : scheduled;
         },
      },
   };
   return {
      calls,
      callbackCount: () => callbacks.size,
      deliver(index, message) {
         callbacks.get(CHANNEL_ID)?.({ index, message });
      },
   };
}

function cancels(calls: Call[]): Call[] {
   return calls.filter(({ cmd }) => cmd === 'plugin:media-parser|cancel_thumbnail_stream');
}

function thumbnailMessage(indices: number[]): ArrayBuffer {
   const entry = {
      trackId: 1,
      width: 2,
      height: 2,
      timestampSec: 0,
      format: 'jpeg',
      mimeType: 'image/jpeg',
      offset: 0,
      length: 1,
      indices,
   };
   const header = new TextEncoder().encode(JSON.stringify({ version: 1, entries: [entry] }));
   const message = new Uint8Array(4 + header.length + 1);
   new DataView(message.buffer).setUint32(0, header.length, true);
   message.set(header, 4);
   message[4 + header.length] = 0xff;
   return message.buffer;
}

const tick = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

test('resolves only after reordered messages and the terminal answer every position', async () => {
   const ipc = mockIpc();
   const seen: number[][] = [];
   let resolved = false;

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500, 0],
      onThumbnail: (thumbnail: StreamedThumbnailInfo) => {
         seen.push(thumbnail.indices);
      },
   }).then(() => {
      resolved = true;
   });
   await tick();
   ipc.deliver(2, { done: 2 });
   ipc.deliver(1, thumbnailMessage([1]));
   await tick();
   assert.equal(resolved, false, 'the terminal arrived, but a message is still missing');

   ipc.deliver(0, thumbnailMessage([0, 2]));
   await stream;
   assert.deepEqual(seen, [[0, 2], [1]]);
   assert.deepEqual(cancels(ipc.calls), [], 'a finished stream has nothing to cancel');
});

test('resolves an empty request on its terminal alone', async () => {
   const ipc = mockIpc();

   const stream = streamThumbnails('/video.mp4', { timestamps: [], onThumbnail: () => assert.fail() });
   ipc.deliver(0, { done: 0 });

   await stream;
});

test('accepts thumbnails delivered as byte arrays or typed arrays', async () => {
   const ipc = mockIpc();
   const seen: number[][] = [];

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: (thumbnail) => {
         seen.push(thumbnail.indices);
      },
   });
   ipc.deliver(0, Array.from(new Uint8Array(thumbnailMessage([0]))));
   ipc.deliver(1, new Uint8Array(thumbnailMessage([1])));
   ipc.deliver(2, { done: 2 });

   await stream;
   assert.deepEqual(seen, [[0], [1]]);
});

test('a throwing callback rejects, stops later deliveries, and cancels the backend once', async () => {
   const ipc = mockIpc();
   let calls = 0;

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => {
         calls += 1;
         throw new Error('consumer failed');
      },
   });
   ipc.deliver(0, thumbnailMessage([0]));
   ipc.deliver(1, thumbnailMessage([1]));
   ipc.deliver(2, { done: 2 });

   await assert.rejects(stream, /consumer failed/);
   assert.equal(calls, 1);
   assert.deepEqual(cancels(ipc.calls), [
      { cmd: 'plugin:media-parser|cancel_thumbnail_stream', args: { channelId: CHANNEL_ID } },
   ]);
});

test('rejects with the original error even when the cancel request fails', async () => {
   const unhandled: unknown[] = [];
   const record = (reason: unknown): void => {
      unhandled.push(reason);
   };
   process.on('unhandledRejection', record);
   try {
      const ipc = mockIpc(Promise.resolve(null), Promise.reject(new Error('cancel failed')));

      const stream = streamThumbnails('/video.mp4', {
         timestamps: [0],
         onThumbnail: () => {
            throw new Error('consumer failed');
         },
      });
      ipc.deliver(0, thumbnailMessage([0]));

      await assert.rejects(stream, /consumer failed/);
      await tick();
      assert.deepEqual(unhandled, []);
   } finally {
      process.off('unhandledRejection', record);
   }
});

test('rejects a position answered twice', async () => {
   const ipc = mockIpc();

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => undefined,
   });
   ipc.deliver(0, thumbnailMessage([0]));
   ipc.deliver(1, thumbnailMessage([0]));

   await assert.rejects(stream, /invalid index: 0/);
});

test('rejects when a position is never answered', async () => {
   const ipc = mockIpc();

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => undefined,
   });
   ipc.deliver(0, thumbnailMessage([0]));
   ipc.deliver(1, { done: 1 });

   await assert.rejects(stream, /every timestamp exactly once/);
});

test('rejects when the terminal count disagrees with the messages received', async () => {
   const ipc = mockIpc();

   // Every position is answered, so only the message count can reject.
   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => undefined,
   });
   ipc.deliver(0, thumbnailMessage([0]));
   ipc.deliver(1, thumbnailMessage([1]));
   ipc.deliver(2, { done: 1 });

   await assert.rejects(stream, /every timestamp exactly once/);
});

test('rejects with the serialized backend error from the terminal', async () => {
   const ipc = mockIpc();

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => assert.fail(),
   });
   ipc.deliver(0, { error: 'Media parser error: Operation cancelled' });

   await assert.rejects(stream, (reason) => reason === 'Media parser error: Operation cancelled');
});

for (const message of [{ done: -1 }, { done: 1.5 }, { done: 0, extra: true }, { error: 1 }, 'done', null]) {
   test(`rejects an unexpected stream message: ${JSON.stringify(message)}`, async () => {
      const ipc = mockIpc();

      const stream = streamThumbnails('/video.mp4', {
         timestamps: [0],
         onThumbnail: () => assert.fail(),
      });
      ipc.deliver(0, message);

      await assert.rejects(stream, TypeError);
   });
}

test('rejects with the backend error when invoke rejects', async () => {
   const error = new Error('backend failed');
   mockIpc(Promise.reject(error));

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => assert.fail(),
   });

   await assert.rejects(stream, (reason) => reason === error);
});

test('rejects on a bad message while the extraction is still running', async () => {
   const ipc = mockIpc();
   let calls = 0;

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => {
         calls += 1;
      },
   });
   ipc.deliver(0, thumbnailMessage([0]));
   ipc.deliver(1, {});

   await assert.rejects(stream, TypeError);
   assert.equal(calls, 1);
   assert.deepEqual(cancels(ipc.calls), [
      { cmd: 'plugin:media-parser|cancel_thumbnail_stream', args: { channelId: CHANNEL_ID } },
   ]);
});

test('unregisters channels when invoke rejects before entering the handler', async () => {
   const error = new Error('not allowed by ACL');
   const ipc = mockIpc(Promise.reject(error));

   for (let attempt = 0; attempt < 100; attempt += 1) {
      await assert.rejects(streamThumbnails('/video.mp4', {
         timestamps: [0, 500],
         onThumbnail: () => assert.fail(),
      }), (reason) => reason === error);
   }
   assert.equal(ipc.callbackCount(), 0);
});
