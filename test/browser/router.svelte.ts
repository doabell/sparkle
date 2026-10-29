export const page = $state({
    params: { id: "1" },
    url: new URL("https://sparkle.test/playlists/1"),
});
export const navigations: string[] = [];
export const goto = async (url: string) => {
    navigations.push(url);
};

export function navigate(id: number) {
    page.params = { id: String(id) };
    page.url = new URL(`https://sparkle.test/playlists/${id}`);
}
