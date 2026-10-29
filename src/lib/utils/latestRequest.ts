/** Invalidates async work when a newer request starts or its owner unmounts. */
export function createLatestRequest() {
    let revision = 0;
    return {
        start() {
            const request = ++revision;
            return () => request === revision;
        },
        invalidate() {
            revision++;
        },
    };
}
