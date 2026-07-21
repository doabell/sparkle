import { convertFileSrc } from "@tauri-apps/api/core";

export function bytesToDataUrl(bytes: number[], mimeType: string): string {
    return `data:${mimeType};base64,${bytesToBase64(bytes)}`;
}

export function bytesToBase64(bytes: number[]): string {
    const binary = bytesToBinary(bytes);
    return "btoa" in globalThis ? btoa(binary) : btoaFallback(binary);
}

function bytesToBinary(bytes: number[]): string {
    const maxChunkSize = 65535;
    let binary = "";
    for (let i = 0; i < bytes.length; i += maxChunkSize) {
        const chunk = bytes.slice(i, i + maxChunkSize);
        binary += String.fromCharCode(...chunk);
    }
    return binary;
}

function btoaFallback(binary: string): string {
    const chars =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let result = "";
    let i = 0;
    while (i < binary.length) {
        const a = binary.charCodeAt(i++);
        const b = i < binary.length ? binary.charCodeAt(i++) : 0;
        const c = i < binary.length ? binary.charCodeAt(i++) : 0;
        const bitmap = (a << 16) | (b << 8) | c;
        result += chars.charAt((bitmap >> 18) & 63);
        result += chars.charAt((bitmap >> 12) & 63);
        result +=
            i - 2 < binary.length ? chars.charAt((bitmap >> 6) & 63) : "=";
        result += i - 1 < binary.length ? chars.charAt(bitmap & 63) : "=";
    }
    return result;
}

// Keep the encoded URL tied to the image object. Unlike object URLs, data URLs
// need no explicit revocation: once a bounded image cache and its consumers
// release the ImageData object, the encoded string can be collected with it.
const urlCache = new WeakMap<object, string>();

/**
 * Cached album and artist images stay on disk and are served through Tauri's
 * scoped asset protocol. This avoids serializing image bytes into JavaScript
 * arrays and then allocating a second base64 copy for every visible card.
 */
export function cachedImageToUrl(
    image: { file_path?: string },
    fallback = "",
): string {
    return image.file_path ? convertFileSrc(image.file_path) : fallback;
}

export function imageDataToUrl(
    image: { source: string; data?: number[]; mime_type?: string },
    fallback = "",
): string {
    if (!image.data || image.data.length === 0) return fallback;
    const cached = urlCache.get(image);
    if (cached) return cached;
    const url = bytesToDataUrl(image.data, image.mime_type || "image/jpeg");
    urlCache.set(image, url);
    return url;
}
