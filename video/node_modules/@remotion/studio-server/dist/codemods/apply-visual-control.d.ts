import type { File } from '@babel/types';
import { type ApplyVisualControlCodemod } from '@remotion/studio-shared';
import type { ApplyCodeModReturnType, Change } from './recast-mods';
export declare const applyVisualControl: ({ file, transformation, changesMade, }: {
    file: File;
    transformation: ApplyVisualControlCodemod;
    changesMade: Change[];
}) => ApplyCodeModReturnType;
