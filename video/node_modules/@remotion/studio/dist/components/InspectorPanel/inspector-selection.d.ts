import { type TimelineSelection } from '../Timeline/TimelineSelection';
export type SequenceSectionSelection = Extract<TimelineSelection, {
    type: 'sequence' | 'sequence-prop' | 'sequence-all-effects' | 'sequence-effect' | 'sequence-effect-prop';
}>;
export declare const isSequenceSectionSelection: (selection: TimelineSelection) => selection is SequenceSectionSelection;
type SameSequenceInspectorSelection = Extract<TimelineSelection, {
    type: 'sequence-prop' | 'sequence-effect' | 'sequence-effect-prop';
}>;
export declare const getSameSequenceInspectorSelection: (selections: readonly TimelineSelection[]) => SameSequenceInspectorSelection | null;
export {};
