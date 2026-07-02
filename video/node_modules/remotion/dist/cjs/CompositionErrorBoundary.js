"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.CompositionErrorBoundary = void 0;
const react_1 = __importDefault(require("react"));
const getHot = () => {
    var _a;
    try {
        if (typeof __webpack_module__ === 'undefined') {
            return null;
        }
        return (_a = __webpack_module__.hot) !== null && _a !== void 0 ? _a : null;
    }
    catch (_b) {
        return null;
    }
};
class CompositionErrorBoundary extends react_1.default.Component {
    state = { hasError: false };
    hmrStatusHandler = null;
    static getDerivedStateFromError() {
        return { hasError: true };
    }
    componentDidCatch(error) {
        this.props.onError(error);
        this.subscribeToHmrReset();
    }
    componentDidMount() {
        // A fresh boundary mounting in the success state means any stale
        // `renderError` left over by a previous boundary instance should be
        // cleared. Fast Refresh sometimes unmounts the old (errored) boundary
        // and mounts a new one during `apply`, so `componentDidUpdate`'s
        // error→success transition never fires.
        if (!this.state.hasError) {
            this.props.onClear();
        }
    }
    componentDidUpdate(_prevProps, prevState) {
        if (prevState.hasError && !this.state.hasError) {
            this.props.onClear();
        }
    }
    componentWillUnmount() {
        this.unsubscribeFromHmrReset();
    }
    subscribeToHmrReset() {
        if (this.hmrStatusHandler) {
            return;
        }
        const hot = getHot();
        if (!hot) {
            return;
        }
        // Once the boundary catches a runtime error it returns `null` on every
        // render and never retries the children — so any subsequent HMR fix
        // would be invisible. While in the error state, wait for the next
        // time webpack HMR settles (`idle`), drop the error flag, and let the
        // boundary re-render. If the children still throw, `componentDidCatch`
        // resubscribes. If they succeed, `componentDidUpdate` calls
        // `onClear()`. See https://github.com/remotion-dev/remotion/issues/7447.
        const handler = (status) => {
            if (status !== 'idle') {
                return;
            }
            this.unsubscribeFromHmrReset();
            this.setState({ hasError: false });
        };
        this.hmrStatusHandler = handler;
        hot.addStatusHandler(handler);
    }
    unsubscribeFromHmrReset() {
        const handler = this.hmrStatusHandler;
        if (!handler) {
            return;
        }
        this.hmrStatusHandler = null;
        const hot = getHot();
        if (!hot) {
            return;
        }
        hot.removeStatusHandler(handler);
    }
    render() {
        if (this.state.hasError) {
            return null;
        }
        return this.props.children;
    }
}
exports.CompositionErrorBoundary = CompositionErrorBoundary;
