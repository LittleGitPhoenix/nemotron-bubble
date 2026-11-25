# Named click targets of the settings window: tab and center point in window pixels.
# They mirror the layout constants in src/ui/settings_window/*.rs (window 392 x 816).
# UPDATE THIS TABLE WHENEVER THE LAYOUT CHANGES, then check each changed entry with a screenshot.
#
# Settings tab entries are in content coordinates (the position at scroll offset 0). That tab
# scrolls by up to SETTINGS_MAX_SCROLL px, Invoke-Target.ps1 scrolls it to the top or bottom first.

$script:SettingsTabMaxScroll = 83 # SETTINGS_CONTENT_BOTTOM (863) - SETTINGS_VIEW_BOTTOM (780)
$script:SettingsViewBottom = 780  # rows whose center lies below this need the tab scrolled down

$script:Targets = [ordered]@{
	# frame (all tabs)
	'Close'                = @{ Tab = $null; X = 358; Y = 34 }
	'Tab:History'          = @{ Tab = $null; X = 78; Y = 93 }
	'Tab:Model'            = @{ Tab = $null; X = 195; Y = 93 }
	'Tab:Settings'         = @{ Tab = $null; X = 312; Y = 93 }

	# History tab (copy icons move with the list, only the first one is fixed)
	'Trash'                = @{ Tab = 'History'; X = 336; Y = 142 }
	'Copy:First'           = @{ Tab = 'History'; X = 46; Y = 177 }

	# Model tab
	'Processing:CPU'       = @{ Tab = 'Model'; X = 92; Y = 192 }
	'Processing:GPU'       = @{ Tab = 'Model'; X = 196; Y = 192 }
	'Processing:Auto'      = @{ Tab = 'Model'; X = 300; Y = 192 }
	'Model:English'        = @{ Tab = 'Model'; X = 92; Y = 290 }
	'Model:Multilingual'   = @{ Tab = 'Model'; X = 196; Y = 290 }
	'Model:Predefined'     = @{ Tab = 'Model'; X = 300; Y = 290 }
	'CheckForUpdates'      = @{ Tab = 'Model'; X = 277; Y = 348 }
	'Download:English'     = @{ Tab = 'Model'; X = 302; Y = 394 } # Download / Update / Resume / Retry / Cancel
	'Download:Multilingual' = @{ Tab = 'Model'; X = 302; Y = 450 }
	'Language'             = @{ Tab = 'Model'; X = 282; Y = 518 } # opens a popup menu
	'Toggle:Preload'       = @{ Tab = 'Model'; X = 329; Y = 576 }

	# Settings tab (content coordinates)
	'Shortcut'             = @{ Tab = 'Settings'; X = 277; Y = 147 } # starts or cancels the key capture
	'Microphone'           = @{ Tab = 'Settings'; X = 196; Y = 240 } # opens a popup menu
	'BubblePosition'       = @{ Tab = 'Settings'; X = 277; Y = 835 } # opens a popup menu
}

# The switches of the Settings tab, in SETTINGS_TAB_TOGGLES order: row top 274 + 49 * index,
# switch center 22 px below the row top.
$toggleNames = @(
	'StartWithWindows', 'LiveType', 'CopyFinal', 'PasteFinal', 'Sounds', 'Waveform',
	'TrayAnimation', 'EnterStops', 'SilenceStop', 'FloatingBubble', 'BubbleClickOpensSettings'
)
for ($i = 0; $i -lt $toggleNames.Count; $i++)
{
	$script:Targets["Toggle:$($toggleNames[$i])"] = @{ Tab = 'Settings'; X = 329; Y = 274 + 49 * $i + 22 }
}
