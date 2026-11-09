import { MenuItem } from "@decky/ui";

import { fetchAndApplyArtwork } from "./artwork";
import { t } from "./i18n";
import { metadataCache, appName } from "./steam";
import { toastError, toastSuccess } from "./toast";

const applyForApp = async (appId: number) => {
  try {
    const count = await fetchAndApplyArtwork(
      appId,
      appName(appId),
      Number(metadataCache[String(appId)]?.steam_appid) || null,
    );
    toastSuccess(t("qam.artwork.title"), t("toast.artwork.applied", { count }));
  } catch (error) {
    toastError(t("qam.artwork.title"), String(error));
  }
};

/** SteamGridDB artwork entry for the library context menu. */
export const artworkMenuItem = (appId: number) => (
  <MenuItem key="decky-metadata-artwork" onSelected={() => { void applyForApp(appId); }}>
    {t("contextMenu.artwork")}
  </MenuItem>
);
