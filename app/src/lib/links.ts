import { invoke } from "@tauri-apps/api/core";
/** Open one of Index's GitHub pages in the browser. */
export const openLink = (url: string) => invoke<void>("open_release_page", { url }).catch(() => {});
