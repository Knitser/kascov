type State = {
    editorShowOutlines: boolean;
    setEditorShowOutlines: (cb: (prevState: boolean) => boolean) => void;
};
export declare const persistEditorShowOutlinesOption: (option: boolean) => void;
export declare const loadEditorShowOutlinesOption: () => boolean;
export declare const EditorShowOutlinesContext: import("react").Context<State>;
export {};
