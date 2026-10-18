@ECHO OFF

REM // Usage:
REM // build.bat Debug ""
REM // build.bat Release :rebuild

SET "VSCMD_START_DIR=%CD%"
IF NOT "%FrameworkVersion%" == "" GOTO build

SET "WH_VS_INSTALL_PATH="
FOR /F "usebackq tokens=*" %%I IN (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) DO SET "WH_VS_INSTALL_PATH=%%I"
IF NOT DEFINED WH_VS_INSTALL_PATH (
    ECHO No Visual Studio installation with C++ build tools was found. 1>&2
    EXIT /b 1
)

SET "WH_VCVARS_ARCH=amd64"
IF /I "%PROCESSOR_ARCHITECTURE%" == "ARM64" SET "WH_VCVARS_ARCH=arm64_x64"
IF /I "%PROCESSOR_ARCHITEW6432%" == "ARM64" SET "WH_VCVARS_ARCH=arm64_x64"
CALL "%WH_VS_INSTALL_PATH%\VC\Auxiliary\Build\vcvarsall.bat" %WH_VCVARS_ARCH% || GOTO fail

:build
MSBuild.exe "windhawk.sln" /m /t:"app%~2" /p:Configuration="%~1" /p:Platform="x64" || GOTO fail
MSBuild.exe "windhawk.sln" /m /t:"engine%~2" /p:Configuration="%~1" /p:Platform="Win32" || GOTO fail
MSBuild.exe "windhawk.sln" /m /t:"engine%~2" /p:Configuration="%~1" /p:Platform="x64" || GOTO fail
MSBuild.exe "windhawk.sln" /m /t:"engine%~2" /p:Configuration="%~1" /p:Platform="ARM64" || GOTO fail

REM // Done
EXIT /b 0

:fail
EXIT /b %ERRORLEVEL%
