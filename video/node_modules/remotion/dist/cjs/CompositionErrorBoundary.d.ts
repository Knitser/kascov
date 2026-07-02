import React from 'react';
type Props = {
    readonly children: React.ReactNode;
    readonly onError: (error: Error) => void;
    readonly onClear: () => void;
};
type State = {
    hasError: boolean;
};
export declare class CompositionErrorBoundary extends React.Component<Props, State> {
    state: State;
    private hmrStatusHandler;
    static getDerivedStateFromError(): Partial<State>;
    componentDidCatch(error: Error): void;
    componentDidMount(): void;
    componentDidUpdate(_prevProps: Props, prevState: State): void;
    componentWillUnmount(): void;
    private subscribeToHmrReset;
    private unsubscribeFromHmrReset;
    render(): React.ReactNode;
}
export {};
