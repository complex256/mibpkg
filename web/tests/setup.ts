import '@testing-library/jest-dom/vitest';

// jsdom lacks a few browser APIs the Radix primitives use.
Object.defineProperty(Element.prototype, 'hasPointerCapture', { value: () => false, configurable: true });
Object.defineProperty(Element.prototype, 'setPointerCapture', { value: () => {}, configurable: true });
Object.defineProperty(Element.prototype, 'releasePointerCapture', { value: () => {}, configurable: true });
Object.defineProperty(Element.prototype, 'scrollIntoView', { value: () => {}, configurable: true });

if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;

window.scrollTo = () => {};
