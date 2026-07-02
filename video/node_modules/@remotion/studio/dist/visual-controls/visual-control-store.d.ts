type Listener = () => void;
export declare const visualControlStore: {
    subscribe(listener: Listener): () => void;
    getSnapshot(): number;
    emitChange(): void;
};
export {};
