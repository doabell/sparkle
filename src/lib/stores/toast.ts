import { writable } from "svelte/store";

export type ToastType = "info" | "success" | "error";

export interface Toast {
    id: number;
    message: string;
    type: ToastType;
}

function createToastStore() {
    const { subscribe, update, set } = writable<Toast[]>([]);
    let nextId = 0;

    function removeToast(id: number) {
        update((toasts) => toasts.filter((t) => t.id !== id));
    }

    return {
        subscribe,
        addToast(message: string, type: ToastType = "info") {
            const id = nextId++;
            update((toasts) => [...toasts, { id, message, type }]);
            setTimeout(() => removeToast(id), 5000);
        },
        removeToast,
        clearToasts() {
            set([]);
        },
    };
}

export const toasts = createToastStore();
export const addToast = toasts.addToast;
export const clearToasts = toasts.clearToasts;
