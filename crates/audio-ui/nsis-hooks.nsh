!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToStack 'taskkill /F /IM audio-ui.exe /T'
  Pop $0
  Pop $1
  nsExec::ExecToStack 'taskkill /F /IM audio-daemon.exe /T'
  Pop $0
  Pop $1
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToStack 'taskkill /F /IM audio-ui.exe /T'
  Pop $0
  Pop $1
  nsExec::ExecToStack 'taskkill /F /IM audio-daemon.exe /T'
  Pop $0
  Pop $1
!macroend
