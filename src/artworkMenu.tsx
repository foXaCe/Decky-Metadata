import { MenuItem, Navigation } from "@decky/ui";

import { t } from "./i18n";

/** Artwork page entry for the library context menu. */
export const artworkMenuItem = (appId: number) => (
  <MenuItem
    key="decky-metadata-artwork"
    onSelected={() => Navigation.Navigate(`/decky-metadata-artwork/${appId}`)}
  >
    {t("contextMenu.artwork")}
  </MenuItem>
);
