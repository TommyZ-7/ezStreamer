Populated at bundle time (CI Release): GStreamer MSVC runtime subset
(bin/*.dll + lib/gstreamer-1.0 plugins). See docs/design.md §13.2.
The backend prefers this directory (PATH + GST_PLUGIN_PATH) and falls
back to the system runtime when absent (dev machines).
