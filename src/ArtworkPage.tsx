import { Focusable, Navigation, PanelSection, PanelSectionRow, ScrollPanel, useParams } from "@decky/ui";
import { useCallback, useEffect, useState } from "react";

import { artworkCandidates, applyArtworkAsset } from "./artwork";
import type { ArtworkCandidate, ArtworkCandidates } from "./backend";
import { t } from "./i18n";
import type { MessageKey } from "./i18n";
import { appName, metadataCache } from "./steam";
import { compactTextStyle, FocusableButton, pageStyle, pageTitleStyle } from "./styles";
import { toastError, toastSuccess } from "./toast";

type Slot = {
  kind: keyof ArtworkCandidates;
  label: MessageKey;
  type: number;
  width: number;
  height: number;
};

const SLOTS: Slot[] = [
  { kind: "portrait", label: "editor.artwork.slot.portrait", type: 0, width: 120, height: 180 },
  { kind: "landscape", label: "editor.artwork.slot.landscape", type: 3, width: 200, height: 94 },
  { kind: "hero", label: "editor.artwork.slot.hero", type: 1, width: 240, height: 78 },
  { kind: "logo", label: "editor.artwork.slot.logo", type: 2, width: 200, height: 90 },
];

const steamApps = () => (globalThis as any)?.SteamClient?.Apps;

export const ArtworkPage = () => {
  const { appid } = useParams<{ appid: string }>();
  const appId = Number(appid);
  const [slot, setSlot] = useState<Slot>(SLOTS[0]);
  const [candidates, setCandidates] = useState<Partial<ArtworkCandidates> | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const title = appName(appId);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const steamAppId = Number(metadataCache[String(appId)]?.steam_appid) || null;
      setCandidates(await artworkCandidates(appId, title, steamAppId));
    } catch (error) {
      toastError(t("qam.artwork.title"), String(error));
    } finally {
      setLoading(false);
    }
  }, [appId, title]);

  useEffect(() => {
    void load();
  }, [load]);

  const apply = async (candidate: ArtworkCandidate) => {
    if (busy) return;
    setBusy(true);
    try {
      await applyArtworkAsset(appId, slot.kind, candidate.url);
      toastSuccess(t("editor.artwork.applied"), "");
    } catch (error) {
      toastError(t("qam.artwork.title"), String(error));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    if (busy) return;
    const api = steamApps();
    if (!api?.ClearCustomArtworkForApp) return;
    setBusy(true);
    try {
      await api.ClearCustomArtworkForApp(appId, slot.type);
      await new Promise((resolve) => window.setTimeout(resolve, 500));
      toastSuccess(t("editor.artwork.cleared"), "");
    } catch (error) {
      toastError(t("qam.artwork.title"), String(error));
    } finally {
      setBusy(false);
    }
  };

  const items = candidates?.[slot.kind] ?? [];

  return (
    <ScrollPanel>
      <div style={pageStyle}>
        <Focusable className="DialogButton" onActivate={() => Navigation.NavigateBack()} style={pageTitleStyle}>
          {t("editor.artwork.pageTitle", { game: title })}
        </Focusable>
        <PanelSection>
          <PanelSectionRow>
            <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
              {SLOTS.map((entry) => (
                <FocusableButton
                  key={entry.kind}
                  className="DialogButton"
                  onClick={() => setSlot(entry)}
                  style={entry.kind === slot.kind ? { outline: "2px solid #1a9fff" } : undefined}
                >
                  {t(entry.label)}
                </FocusableButton>
              ))}
            </div>
          </PanelSectionRow>
        </PanelSection>
        <PanelSection>
          {loading ? (
            <PanelSectionRow>
              <div style={compactTextStyle}>{t("editor.artwork.loading")}</div>
            </PanelSectionRow>
          ) : items.length === 0 ? (
            <PanelSectionRow>
              <div style={compactTextStyle}>{t("editor.artwork.empty")}</div>
            </PanelSectionRow>
          ) : (
            <PanelSectionRow>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
                {items.map((candidate) => (
                  <FocusableButton
                    key={candidate.url}
                    className="DialogButton"
                    disabled={busy}
                    onClick={() => void apply(candidate)}
                    style={{ padding: 0, minWidth: slot.width, height: slot.height }}
                  >
                    <img
                      src={candidate.thumb || candidate.url}
                      alt=""
                      style={{ width: slot.width, height: slot.height, objectFit: "contain" }}
                    />
                  </FocusableButton>
                ))}
              </div>
            </PanelSectionRow>
          )}
        </PanelSection>
        <PanelSection>
          <PanelSectionRow>
            <FocusableButton className="DialogButton" disabled={busy} onClick={() => void load()}>
              {t("editor.artwork.reload")}
            </FocusableButton>
            <FocusableButton className="DialogButton" disabled={busy} onClick={() => void clear()}>
              {t("editor.artwork.clear")}
            </FocusableButton>
          </PanelSectionRow>
        </PanelSection>
      </div>
    </ScrollPanel>
  );
};
