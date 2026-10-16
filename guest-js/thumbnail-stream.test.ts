import assert from 'node:assert/strict';
import { test } from 'node:test';

import { streamThumbnails, type StreamedThumbnailInfo } from './index';

type RawMessage = { index: number; message: ArrayBuffer };

/**
 * Stands in for the Tauri webview IPC: `invoke` resolves with `sent`, and the
 * test pushes channel messages, in any order, through `deliver`.
 */
function mockIpc(sent: number | Promise<number>): { deliver(index: number, indices: number[]): void } {
   let callback: ((raw: RawMessage) => void) | undefined;
   (globalThis as Record<string, unknown>).window = {
      __TAURI_INTERNALS__: {
         transformCallback(fn: (raw: RawMessage) => void): number {
            callback = fn;
            return 1;
         },
         unregisterCallback(): void {},
         invoke: (): Promise<number> => Promise.resolve(sent),
      },
   };
   return {
      deliver(index, indices) {
         callback?.({ index, message: thumbnailMessage(indices) });
      },
   };
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

test('resolves only after late, reordered messages answer every position', async () => {
   const ipc = mockIpc(2);
   const seen: number[][] = [];
   let resolved = false;

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500, 0],
      onThumbnail: (thumbnail: StreamedThumbnailInfo) => seen.push(thumbnail.indices),
   }).then(() => {
      resolved = true;
   });
   await tick();
   ipc.deliver(1, [1]);
   await tick();
   assert.equal(resolved, false, 'invoke resolved, but a message is still missing');

   ipc.deliver(0, [0, 2]);
   await stream;
   assert.deepEqual(seen, [[0, 2], [1]]);
});

test('resolves an empty request without any message', async () => {
   mockIpc(0);
   await streamThumbnails('/video.mp4', { timestamps: [], onThumbnail: () => assert.fail() });
});

test('a throwing callback rejects the stream and stops later deliveries', async () => {
   const ipc = mockIpc(2);
   let calls = 0;

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => {
         calls += 1;
         throw new Error('consumer failed');
      },
   });
   ipc.deliver(0, [0]);
   ipc.deliver(1, [1]);

   await assert.rejects(stream, /consumer failed/);
   assert.equal(calls, 1);
});

test('rejects a position answered twice', async () => {
   const ipc = mockIpc(2);

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => undefined,
   });
   ipc.deliver(0, [0]);
   ipc.deliver(1, [0]);

   await assert.rejects(stream, /invalid index: 0/);
});

test('rejects when the backend reports more messages than positions answered', async () => {
   const ipc = mockIpc(1);

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => undefined,
   });
   ipc.deliver(0, [0]);

   await assert.rejects(stream, /every timestamp exactly once/);
});

test('rejects on a bad message while the command is still running', async () => {
   const ipc = mockIpc(new Promise<number>(() => undefined));

   const stream = streamThumbnails('/video.mp4', {
      timestamps: [0, 500],
      onThumbnail: () => {
         throw new Error('consumer failed');
      },
   });
   ipc.deliver(0, [0]);

   await assert.rejects(stream, /consumer failed/);
});
