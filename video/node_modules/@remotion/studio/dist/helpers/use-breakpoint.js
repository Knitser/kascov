"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.useBreakpoint = useBreakpoint;
const react_1 = require("react");
function useBreakpoint(breakpoint) {
    const [compactUI, setCompactUI] = (0, react_1.useState)(window.innerWidth < breakpoint);
    const compactUIRef = (0, react_1.useRef)(compactUI);
    (0, react_1.useEffect)(() => {
        function handleResize() {
            const newValue = window.innerWidth < breakpoint;
            if (newValue !== compactUIRef.current) {
                setCompactUI(newValue);
            }
            compactUIRef.current = newValue;
        }
        window.addEventListener('resize', handleResize);
        handleResize();
        return () => window.removeEventListener('resize', handleResize);
    }, [breakpoint]);
    return compactUI;
}
