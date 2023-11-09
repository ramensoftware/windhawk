@ECHO OFF

REM // Usage:
REM // build.bat Debug ""
REM // build.bat Release :rebuild

SET "VSCMD_START_DIR=%CD%"
IF "%FrameworkVersion%" == "" CALL "%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"

MSBuild.exe "windhawk.sln" /m /t:"app%~2" /p:Configuration="%~1" /p:Platform="Win32" || GOTO fail
MSBuild.exe "windhawk.sln" /m /t:"engine%~2" /p:Configuration="%~1" /p:Platform="Win32" || GOTO fail
MSBuild.exe "windhawk.sln" /m /t:"engine%~2" /p:Configuration="%~1" /p:Platform="x64" || GOTO fail

REM // Done
EXIT /b 0

:fail
EXIT /b %ERRORLEVEL%
