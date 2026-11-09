/**
 * SteamGridDB artwork for non-Steam shortcuts.
 *
 * The backend resolves the game with the user's SteamGridDB key and returns the
 * images as base64; Steam's own `SetCustomArtworkForApp` writes the files and
 * invalidates the library cache, so the plugin never touches `config/grid`.
 */
import { fetchArtwork } from "./backend";
import * as log from "./log";

/** Steam artwork slots: 0 portrait grid, 1 hero, 2 logo, 3 landscape grid. */
const SLOTS: Array<[string, number]> = [
  ["portrait", 0],
  ["hero", 1],
  ["logo", 2],
  ["landscape", 3],
];

export type ArtworkPayload = Partial<Record<"portrait" | "landscape" | "hero" | "logo", string>>;

type SteamArtworkApi = {
  SetCustomArtworkForApp?: (appId: number, data: string, format: string, type: number) => Promise<void>;
};

const steamApps = (): SteamArtworkApi | undefined =>
  (globalThis as any)?.SteamClient?.Apps;

/** Hand the downloaded images to Steam for one shortcut. */
export const applyArtwork = async (appId: number, payload: ArtworkPayload): Promise<number> => {
  const api = steamApps();
  if (!api?.SetCustomArtworkForApp) {
    throw new Error("Steam artwork API is unavailable");
  }
  let applied = 0;
  for (const [key, type] of SLOTS) {
    const data = payload[key as keyof ArtworkPayload];
    if (!data) continue;
    await api.SetCustomArtworkForApp(appId, data, "png", type);
    applied += 1;
  }
  if (applied > 0 && payload.logo) {
    // Shortcuts render a blank logo until a position entry exists.
    try {
      const overview = (globalThis as any)?.appStore?.GetAppOverviewByAppID?.(appId);
      await (globalThis as any)?.appDetailsStore?.SaveCustomLogoPosition?.(overview, {
        pinnedPosition: "BottomLeft",
        nWidthPct: 50,
        nHeightPct: 50,
      });
    } catch (error) {
      log.warn("artwork", "logo position save failed", error);
    }
  }
  return applied;
};

/** Download SteamGridDB artwork for one shortcut and apply it. */
export const fetchAndApplyArtwork = async (
  appId: number,
  title: string,
  steamAppId?: number | null,
): Promise<number> => {
  const payload = await fetchArtwork(appId, title, steamAppId ?? null);
  const applied = await applyArtwork(appId, payload as ArtworkPayload);
  if (applied === 0) {
    throw new Error("No artwork found");
  }
  return applied;
};
