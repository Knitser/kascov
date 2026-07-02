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

export { }
export as namespace MediabunnyAacEncoder;
