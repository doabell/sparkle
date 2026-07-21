export function addRecentSearch(
    history: readonly string[],
    query: string,
    maxEntries = 10,
): string[] {
    const normalized = query.trim();
    if (!normalized) return [...history];

    return [
        normalized,
        ...history.filter(
            (item) => item.toLowerCase() !== normalized.toLowerCase(),
        ),
    ].slice(0, maxEntries);
}
