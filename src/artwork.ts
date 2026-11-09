/**
 * SteamGridDB artwork for non-Steam shortcuts.
 *
 * The backend resolves the game with the user's SteamGridDB key and returns the
 * images as base64; Steam's own `SetCustomArtworkForApp` writes the files and
 * invalidates the library cache, so the plugin never touches `config/grid`.
 */
import { downloadArtwork, fetchArtwork, getArtworkStatus, listArtwork } from "./backend";
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

/** Apply one chosen gallery image to one Steam slot. */
export const applyArtworkAsset = async (
  appId: number,
  kind: keyof ArtworkPayload,
  url: string,
): Promise<void> => {
  const api = steamApps();
  if (!api?.SetCustomArtworkForApp) {
    throw new Error("Steam artwork API is unavailable");
  }
  const slot = SLOTS.find(([key]) => key === kind);
  if (!slot) throw new Error("Unknown artwork slot");
  const data = await downloadArtwork(url);
  if (!data) throw new Error("Artwork download failed");
  await api.SetCustomArtworkForApp(appId, data, "png", slot[1]);
};

/** Gallery candidates for every Steam slot. */
export const artworkCandidates = async (
  appId: number,
  title: string,
  steamAppId?: number | null,
) => listArtwork(appId, title, steamAppId ?? null);

/** Shortcuts with at least one missing artwork slot. */
export const gamesMissingArtwork = async (appIds: number[]): Promise<number[]> => {
  if (appIds.length === 0) return [];
  const status = await getArtworkStatus(appIds);
  return appIds.filter((appId) => {
    const entry = status[String(appId)];
    return !entry || !entry.portrait || !entry.landscape || !entry.hero || !entry.logo;
  });
};

/**
 * Apply the best artwork to every shortcut that misses at least one slot.
 * Returns how many games received images.
 */
export const applyArtworkToMissing = async (
  games: Array<{ appId: number; title: string; steamAppId?: number | null }>,
  onProgress?: (done: number, total: number) => void,
): Promise<number> => {
  const missing = await gamesMissingArtwork(games.map((game) => game.appId));
  let applied = 0;
  for (const [index, appId] of missing.entries()) {
    const game = games.find((candidate) => candidate.appId === appId);
    if (!game) continue;
    onProgress?.(index + 1, missing.length);
    try {
      applied += await fetchAndApplyArtwork(appId, game.title, game.steamAppId);
    } catch (error) {
      log.warn("artwork", `artwork failed for ${appId}`, error);
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
