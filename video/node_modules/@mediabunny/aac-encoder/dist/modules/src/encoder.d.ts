/*!
 * Copyright (c) 2026-present, Vanilagy and contributors
 *
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
/**
 * Registers the AAC encoder, which Mediabunny will then use automatically when applicable. Make sure to call this
 * function before starting any encoding task.
 *
 * Preferably, wrap the call in a condition to avoid overriding any native AAC encoder:
 *
 * ```ts
 * import { canEncodeAudio } from 'mediabunny';
 * import { registerAacEncoder } from '@mediabunny/aac-encoder';
 *
 * if (!(await canEncodeAudio('aac'))) {
 *     registerAacEncoder();
 * }
 * ```
 *
 * @group \@mediabunny/aac-encoder
 * @public
 */
export declare const registerAacEncoder: () => void;
//# sourceMappingURL=encoder.d.ts.map