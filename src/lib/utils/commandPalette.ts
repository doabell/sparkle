export function moveCommandSelection(
    currentIndex: number,
    delta: number,
    selectableCount: number,
): number {
    const lastIndex = Math.max(0, selectableCount - 1);
    return Math.max(0, Math.min(currentIndex + delta, lastIndex));
}
