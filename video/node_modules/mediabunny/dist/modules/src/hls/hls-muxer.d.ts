/*!
 * Copyright (c) 2026-present, Vanilagy and contributors
 *
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
import { AsyncMutex } from '../misc.js';
import { Muxer } from '../muxer.js';
import { Output, OutputAudioTrack, OutputSubtitleTrack, OutputTrack, OutputVideoTrack } from '../output.js';
import { HlsOutputFormat, HlsOutputFormatOptions, HlsOutputSegmentInfo, OutputFormat } from '../output-format.js';
import { EncodedPacket } from '../packet.js';
import { SubtitleCue, SubtitleMetadata } from '../subtitles.js';
import { Target } from '../target.js';
type HlsTrackData = {
    track: OutputTrack;
    packets: EncodedPacket[];
    playlist: Playlist;
    closed: boolean;
    info: {
        type: 'video';
        decoderConfig: VideoDecoderConfig;
    } | {
        type: 'audio';
        decoderConfig: AudioDecoderConfig;
    };
};
type HlsVideoTrackData = HlsTrackData & {
    info: {
        type: 'video';
    };
};
type HlsAudioTrackData = HlsTrackData & {
    info: {
        type: 'audio';
    };
};
type PlaylistSegment = {
    path: string;
    duration: number;
    timestamp: number;
    byteSize: number;
    byteOffset: number | null;
    info: HlsOutputSegmentInfo | null;
};
type Playlist = {
    id: number;
    path: string;
    tracks: OutputTrack[];
    segmentFormat: OutputFormat;
    currentSegmentStartTimestamp: number | null;
    currentSegmentStartTimestampIsFixed: boolean;
    nextSegmentId: number;
    initSegment: PlaylistSegment | null;
    writtenSegments: PlaylistSegment[];
    peakBitrate: number | null;
    averageBitrate: number | null;
    mediaSequence: number;
    done: boolean;
    singleFile: {
        target: Target;
        path: string;
        nextOffset: number;
        info: HlsOutputSegmentInfo;
    } | null;
    mutex: AsyncMutex;
};
type PlaylistDeclaration = {
    playlist: Playlist;
    groupId: string | null;
    noUri: boolean;
    references: PlaylistDeclaration[];
};
export declare class HlsMuxer extends Muxer {
    format: HlsOutputFormat;
    getPlaylistPath: NonNullable<HlsOutputFormatOptions['getPlaylistPath']>;
    getSegmentPath: NonNullable<HlsOutputFormatOptions['getSegmentPath']>;
    getInitPath: NonNullable<HlsOutputFormatOptions['getInitPath']>;
    targetSegmentDuration: number;
    trackDatas: HlsTrackData[];
    singleFilePerPlaylist: boolean;
    isLive: boolean;
    maxLiveSegmentCount: number;
    isRelativeToUnixEpoch: boolean;
    globalTargetDuration: number;
    numWrittenMasterPlaylists: number;
    playlists: Playlist[];
    playlistDeclarations: PlaylistDeclaration[];
    constructor(output: Output, format: HlsOutputFormat);
    start(): Promise<void>;
    getMimeType(): Promise<string>;
    private allTracksAreKnown;
    onTrackClose(track: OutputTrack): Promise<void>;
    getVideoTrackData(track: OutputVideoTrack, meta?: EncodedVideoChunkMetadata): HlsVideoTrackData;
    getAudioTrackData(track: OutputAudioTrack, meta?: EncodedAudioChunkMetadata): HlsAudioTrackData;
    addEncodedVideoPacket(track: OutputVideoTrack, packet: EncodedPacket, meta?: EncodedVideoChunkMetadata): Promise<void>;
    addEncodedAudioPacket(track: OutputAudioTrack, packet: EncodedPacket, meta?: EncodedAudioChunkMetadata): Promise<void>;
    addSubtitleCue(track: OutputSubtitleTrack, cue: SubtitleCue, meta?: SubtitleMetadata): Promise<void>;
    advancePlaylist(playlist: Playlist): Promise<void>;
    private onPlaylistDone;
    private updatePlaylistBitrates;
    private writePlaylist;
    private writeMasterPlaylist;
    private tryWriteMasterPlaylist;
    finalize(): Promise<void>;
}
export {};
//# sourceMappingURL=hls-muxer.d.ts.map