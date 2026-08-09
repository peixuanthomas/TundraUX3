-- This is a deliberately inert, installation-owned config file.  The bundled
-- Tundra WezTerm fork applies the actual kiosk policy before configuration is
-- observed and rejects user configuration.  Keeping this file in the runtime
-- gives the launcher a concrete, preflight-checked private config path without
-- ever consulting ~/.wezterm.lua or a system configuration.
local wezterm = require 'wezterm'

return {
  automatically_reload_config = false,
  enable_tab_bar = false,
  window_decorations = 'NONE',
}
