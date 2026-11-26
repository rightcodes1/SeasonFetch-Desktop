; Custom NSIS hooks for SeasonFetch Desktop (Windows)
; Preserves local user profile settings & DPAPI auth vault across in-place automatic updates.
!macro customInstall
  DetailPrint "SeasonFetch Desktop installed cleanly."
!macroend

!macro customUnInstall
  DetailPrint "SeasonFetch Desktop removed. Local user preferences in %APPDATA%\SeasonFetch are preserved unless removed manually."
!macroend
