!macro NSIS_HOOK_PREINSTALL
  ; Force-include WebView2Loader.dll for installer builds when bundler resource
  ; collection does not follow the custom cargo target directory.
  File "/oname=WebView2Loader.dll" "C:\\Postgraduate\\Project\\GITHUB\\Codex router\\Codex-Manager\\apps\\gen\\WebView2Loader.dll"
!macroend
