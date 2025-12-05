#pragma once

// Version
#define VERSION_MAJOR               1
#define VERSION_MINOR               7
#define VERSION_REVISION            0
#define VERSION_BUILD               0

// etc.
#define STRINGIZE2(s)               #s
#define STRINGIZE(s)                STRINGIZE2(s)
#define W2(x)                       L ## x
#define W(x)                        W2(x)
#define N(x)                        x
#define LONG_ID(a,b,c,d)            (((a)<<24) | ((b)<<16) | ((c)<<8) | (d))

#define VER_FILE_VERSION            VERSION_MAJOR,VERSION_MINOR,VERSION_REVISION,VERSION_BUILD

#if VERSION_BUILD == 0
#if VERSION_REVISION == 0 // a.b
#define VER_FILE_VERSION_TSTR(t)    t(STRINGIZE(VERSION_MAJOR))    t(".") \
                                    t(STRINGIZE(VERSION_MINOR))
#else // a.b.c
#define VER_FILE_VERSION_TSTR(t)    t(STRINGIZE(VERSION_MAJOR))    t(".") \
                                    t(STRINGIZE(VERSION_MINOR))    t(".") \
                                    t(STRINGIZE(VERSION_REVISION))
#endif // VERSION_REVISION == 0
#else // a.b.c.d
#define VER_FILE_VERSION_TSTR(t)    t(STRINGIZE(VERSION_MAJOR))    t(".") \
                                    t(STRINGIZE(VERSION_MINOR))    t(".") \
                                    t(STRINGIZE(VERSION_REVISION)) t(".") \
                                    t(STRINGIZE(VERSION_BUILD))
#endif // VERSION_BUILD == 0

#define VER_FILE_VERSION_STR        VER_FILE_VERSION_TSTR(N)
#define VER_FILE_VERSION_WSTR       VER_FILE_VERSION_TSTR(W)

#define VER_FILE_VERSION_LONG       LONG_ID(VERSION_MAJOR, VERSION_MINOR, VERSION_REVISION, VERSION_BUILD)
