"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.usePlayerMutedState = exports.useMediaVolumeState = exports.SetMediaVolumeContext = exports.MediaVolumeContext = void 0;
const react_1 = require("react");
exports.MediaVolumeContext = (0, react_1.createContext)({
    playerMuted: false,
    mediaVolume: 1,
});
exports.SetMediaVolumeContext = (0, react_1.createContext)({
    setPlayerMuted: () => {
        throw new Error('default');
    },
    setMediaVolume: () => {
        throw new Error('default');
    },
});
const useMediaVolumeState = () => {
    const { mediaVolume } = (0, react_1.useContext)(exports.MediaVolumeContext);
    const { setMediaVolume } = (0, react_1.useContext)(exports.SetMediaVolumeContext);
    return (0, react_1.useMemo)(() => {
        return [mediaVolume, setMediaVolume];
    }, [mediaVolume, setMediaVolume]);
};
exports.useMediaVolumeState = useMediaVolumeState;
const usePlayerMutedState = () => {
    const { playerMuted } = (0, react_1.useContext)(exports.MediaVolumeContext);
    const { setPlayerMuted } = (0, react_1.useContext)(exports.SetMediaVolumeContext);
    return (0, react_1.useMemo)(() => {
        return [playerMuted, setPlayerMuted];
    }, [playerMuted, setPlayerMuted]);
};
exports.usePlayerMutedState = usePlayerMutedState;
