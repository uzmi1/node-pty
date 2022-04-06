/**
 * Copyright (c) 2013-2015, Christopher Jeffrey, Peter Sunde (MIT License)
 * Copyright (c) 2016, Daniel Imms (MIT License).
 * Copyright (c) 2018, Microsoft Corporation (MIT License).
 *
 * pty.cc:
 *   This file is responsible for starting processes
 *   with pseudo-terminal file descriptors.
 */

#include <cassert>
#include <iostream>
#include <Shlwapi.h> // PathCombine, PathIsRelative
#include <sstream>
#include <string>
#include <vector>
#include <memory>
#include <Windows.h>
#include <strsafe.h>

// Taken from the RS5 Windows SDK, but redefined here in case we're targeting <= 17134
#ifndef PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE
#define PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE \
  ProcThreadAttributeValue(22, FALSE, TRUE, FALSE)
#endif

typedef VOID* HPCON;
typedef HRESULT (__stdcall *PFNCREATEPSEUDOCONSOLE)(COORD c, HANDLE hIn, HANDLE hOut, DWORD dwFlags, HPCON* phpcon);
typedef HRESULT (__stdcall *PFNRESIZEPSEUDOCONSOLE)(HPCON hpc, COORD newSize);
typedef void (__stdcall *PFNCLOSEPSEUDOCONSOLE)(HPCON hpc);

struct pty_baton {
  int id;
  HANDLE hIn;
  HANDLE hOut;
  HPCON hpc;

  pty_baton(int _id, HANDLE _hIn, HANDLE _hOut, HPCON _hpc) : id(_id), hIn(_hIn), hOut(_hOut), hpc(_hpc) {};
};

static std::vector<pty_baton*> ptyHandles;
static volatile LONG ptyCounter;

static pty_baton* get_pty_baton(int id) {
  for (size_t i = 0; i < ptyHandles.size(); ++i) {
    pty_baton* ptyHandle = ptyHandles[i];
    if (ptyHandle->id == id) { return ptyHandle; }
  }
  return nullptr;
}

template <typename T>
std::vector<T> vectorFromString(const std::basic_string<T> &str) {
    return std::vector<T>(str.begin(), str.end());
}

// Returns a new server named pipe.  It has not yet been connected.
bool createDataServerPipe(
    bool write,
    std::wstring kind,
    HANDLE* hServer,
    std::wstring &name,
    const std::wstring &pipeName
    ) {
  *hServer = INVALID_HANDLE_VALUE;

  name = L"\\\\.\\pipe\\" + pipeName + L"-" + kind;

  const DWORD winOpenMode =  PIPE_ACCESS_INBOUND | PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE/*  | FILE_FLAG_OVERLAPPED */;

  SECURITY_ATTRIBUTES sa = {};
  sa.nLength = sizeof(sa);

  *hServer = CreateNamedPipeW(
      name.c_str(),
      /*dwOpenMode=*/winOpenMode,
      /*dwPipeMode=*/PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      /*nMaxInstances=*/1,
      /*nOutBufferSize=*/0,
      /*nInBufferSize=*/0,
      /*nDefaultTimeOut=*/30000,
      &sa);

  return *hServer != INVALID_HANDLE_VALUE;
}

WCHAR *handoff(const std::wstring& ws) {
  size_t sz = sizeof(WCHAR) * (ws.size() + 1);
  WCHAR *pws = (WCHAR*)malloc(sz);
  memcpy(pws, ws.c_str(), sz);
  return pws;
}

HRESULT CreateNamedPipesAndPseudoConsole(COORD size,
                                         DWORD dwFlags,
                                         HANDLE *phInput,
                                         HANDLE *phOutput,
                                         HPCON* phPC,
                                         std::wstring& inName,
                                         std::wstring& outName,
                                         const std::wstring& pipeName)
{
  HANDLE hLibrary = LoadLibraryExW(L"kernel32.dll", 0, 0);
  bool fLoadedDll = hLibrary != nullptr;
  if (fLoadedDll)
  {
    PFNCREATEPSEUDOCONSOLE const pfnCreate = (PFNCREATEPSEUDOCONSOLE)GetProcAddress((HMODULE)hLibrary, "CreatePseudoConsole");
    if (pfnCreate)
    {
      if (phPC == NULL || phInput == NULL || phOutput == NULL)
      {
        return E_INVALIDARG;
      }

      bool success = createDataServerPipe(true, L"in", phInput, inName, pipeName);
      if (!success)
      {
        return HRESULT_FROM_WIN32(GetLastError());
      }
      success = createDataServerPipe(false, L"out", phOutput, outName, pipeName);
      if (!success)
      {
        return HRESULT_FROM_WIN32(GetLastError());
      }
      return pfnCreate(size, *phInput, *phOutput, dwFlags, phPC);
    }
    else
    {
      // Failed to find CreatePseudoConsole in kernel32. This is likely because
      //    the user is not running a build of Windows that supports that API.
      //    We should fall back to winpty in this case.
      return HRESULT_FROM_WIN32(GetLastError());
    }
  }

  // Failed to find  kernel32. This is realy unlikely - honestly no idea how
  //    this is even possible to hit. But if it does happen, fall back to winpty.
  return HRESULT_FROM_WIN32(GetLastError());
}

extern "C"
HRESULT CreateNamedPipesAndPseudoConsole(uint32_t cols, uint32_t rows,
                                         DWORD dwFlags,
                                         WCHAR *ppipeName,
                                         int *pptyId,
                                         void **phIn, WCHAR **pinName,
                                         void **phOut, WCHAR **poutName) {
  HANDLE hIn, hOut;
  HPCON hpc;
  std::wstring inName, outName, pipeName(ppipeName);
  HRESULT hr = CreateNamedPipesAndPseudoConsole({(SHORT) cols, (SHORT) rows},
                                                dwFlags,
                                                &hIn, &hOut, &hpc, inName, outName, pipeName);

  if (SUCCEEDED(hr)) {
    const int ptyId = InterlockedIncrement(&ptyCounter);
    ptyHandles.insert(ptyHandles.end(), new pty_baton(ptyId, hIn, hOut, hpc));

    *pptyId = ptyId;
    *phIn = hIn;
    *pinName = handoff(inName);
    *phOut = hOut;
    *poutName = handoff(outName);
  }

  return hr;
}

int32_t PtyConnect(int id, const std::wstring& cmdline, const std::wstring& cwd, const std::wstring& env,
                   HANDLE& hProcess) {
  // Prepare command line
  std::unique_ptr<wchar_t[]> mutableCommandline = std::make_unique<wchar_t[]>(cmdline.length() + 1);
  HRESULT hr = StringCchCopyW(mutableCommandline.get(), cmdline.length() + 1, cmdline.c_str());
  assert(SUCCEEDED(hr));

  // Prepare cwd
  std::unique_ptr<wchar_t[]> mutableCwd = std::make_unique<wchar_t[]>(cwd.length() + 1);
  hr = StringCchCopyW(mutableCwd.get(), cwd.length() + 1, cwd.c_str());
  assert(SUCCEEDED(hr));

  // Prepare environment
  auto envV = vectorFromString(env);
  LPWSTR envArg = envV.empty() ? nullptr : envV.data();

  // Fetch pty handle from ID and start process
  pty_baton *handle = get_pty_baton(id);

  BOOL success = ConnectNamedPipe(handle->hIn, nullptr) &&
                 ConnectNamedPipe(handle->hOut, nullptr);
  assert(success);

  // Attach the pseudoconsole to the client application we're creating
  STARTUPINFOEXW siEx{0};
  siEx.StartupInfo.cb = sizeof(STARTUPINFOEXW);
  siEx.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
  siEx.StartupInfo.hStdError = nullptr;
  siEx.StartupInfo.hStdInput = nullptr;
  siEx.StartupInfo.hStdOutput = nullptr;

  SIZE_T size = 0;
  InitializeProcThreadAttributeList(NULL, 1, 0, &size);

  BYTE *attrList = new BYTE[size];
  siEx.lpAttributeList = reinterpret_cast<PPROC_THREAD_ATTRIBUTE_LIST>(attrList);

  success = InitializeProcThreadAttributeList(siEx.lpAttributeList, 1, 0, &size);
  if (!success) {
    return -1; // throwNanError(&info, "InitializeProcThreadAttributeList failed", true);
  }

  success = UpdateProcThreadAttribute(siEx.lpAttributeList,
                                       0,
                                       PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                                       handle->hpc,
                                       sizeof(HPCON),
                                       NULL,
                                       NULL);

  if (!success) {
    return -2;
  }

  PROCESS_INFORMATION piClient{};
  success = !!CreateProcessW(
          nullptr,
          mutableCommandline.get(),
          nullptr,                      // lpProcessAttributes
          nullptr,                      // lpThreadAttributes
          false,                        // bInheritHandles VERY IMPORTANT that this is false
          EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, // dwCreationFlags
          envArg,                       // lpEnvironment
          mutableCwd.get(),             // lpCurrentDirectory
          &siEx.StartupInfo,            // lpStartupInfo
          &piClient                     // lpProcessInformation
  );

  if (!success) {
    return -3; // throwNanError(&info, "Cannot create process", true);
  }

  hProcess = piClient.hProcess;
  return piClient.dwProcessId;
}

size_t envlen(const char *env) {
  size_t i = 0;
  while (!(env[i] == 0 && env[i+1] == 0)) i++;
  return i + 2;
}

extern "C"
int32_t PtyConnect(int id, const char *cmdline, const char *cwd, const char *env, HANDLE &hProcess) {
  std::wstring wcmdline(cmdline, cmdline + strlen(cmdline));
  std::wstring wcwd(cwd, cwd + strlen(cwd));
  std::wstring wenv(env, env + envlen(env));

  return PtyConnect(id, wcmdline, wcwd, wenv, hProcess);
}