export type BooleanMap = Record<string, boolean>;
export declare const onlyExpandedBooleanMapValues: (state: BooleanMap) => BooleanMap;
export declare const toggleBooleanMapKey: (state: BooleanMap, key: string) => BooleanMap;
export declare const loadPersistedBooleanMap: (sessionStorageKey: string) => BooleanMap;
export declare const persistBooleanMap: (sessionStorageKey: string, state: BooleanMap) => void;
