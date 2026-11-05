// Kairo's DeckLink output: a small C interface over Blackmagic's DeckLink
// API (C++/COM), called from src-tauri/src/decklink.rs. Lists the cards and
// the video modes each can play in 8-bit BGRA, opens one output (with the
// card's external keyer — fill + key — when asked) and displays frames.
//
// The driver library comes with Blackmagic Desktop Video; with no driver or
// no card, kdl_list reports that and nothing else happens.

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#if defined(_WIN32)
#include <windows.h>
#include <objbase.h>
#include "DeckLinkAPI_h.h"
typedef BOOL dl_bool;
typedef BSTR dl_string;
static std::string to_utf8(dl_string s) {
    if (!s) return "";
    int n = WideCharToMultiByte(CP_UTF8, 0, s, -1, nullptr, 0, nullptr, nullptr);
    std::string out(n > 0 ? n - 1 : 0, '\0');
    if (n > 1) WideCharToMultiByte(CP_UTF8, 0, s, -1, &out[0], n, nullptr, nullptr);
    SysFreeString(s);
    return out;
}
static IDeckLinkIterator* make_iterator() {
    static bool com = false;
    if (!com) { CoInitializeEx(nullptr, COINIT_MULTITHREADED); com = true; }
    IDeckLinkIterator* it = nullptr;
    if (CoCreateInstance(CLSID_CDeckLinkIterator, nullptr, CLSCTX_ALL, IID_IDeckLinkIterator, (void**)&it) != S_OK) return nullptr;
    return it;
}
#elif defined(__APPLE__)
#include <CoreFoundation/CoreFoundation.h>
#include "DeckLinkAPI.h"
typedef bool dl_bool;
typedef CFStringRef dl_string;
static std::string to_utf8(dl_string s) {
    if (!s) return "";
    char buf[512] = {0};
    CFStringGetCString(s, buf, sizeof buf, kCFStringEncodingUTF8);
    CFRelease(s);
    return buf;
}
static IDeckLinkIterator* make_iterator() { return CreateDeckLinkIteratorInstance(); }
#else
#include "DeckLinkAPI.h"
typedef bool dl_bool;
typedef const char* dl_string;
static std::string to_utf8(dl_string s) {
    if (!s) return "";
    std::string out(s);
    free((void*)s);
    return out;
}
static IDeckLinkIterator* make_iterator() { return CreateDeckLinkIteratorInstance(); }
#endif

static std::string json_escape(const std::string& s) {
    std::string o;
    for (char c : s) {
        if (c == '"' || c == '\\') { o += '\\'; o += c; }
        else if ((unsigned char)c < 0x20) o += ' ';
        else o += c;
    }
    return o;
}

static void put(char* out, int cap, const std::string& s) {
    if (!out || cap <= 0) return;
    size_t n = s.size() < (size_t)(cap - 1) ? s.size() : (size_t)(cap - 1);
    memcpy(out, s.data(), n);
    out[n] = 0;
}

static IDeckLink* device_at(int index) {
    IDeckLinkIterator* it = make_iterator();
    if (!it) return nullptr;
    IDeckLink* dl = nullptr;
    IDeckLink* found = nullptr;
    int i = 0;
    while (it->Next(&dl) == S_OK) {
        if (i++ == index) { found = dl; break; }
        dl->Release();
    }
    it->Release();
    return found;
}

extern "C" {

// JSON: {"ok":true,"devices":[{"index","name","keying","modes":[{"id","name","width","height","duration","scale"}]}]}
// or {"ok":false,"error":"..."} when the driver isn't installed.
int kdl_list(char* out, int cap) {
    IDeckLinkIterator* it = make_iterator();
    if (!it) {
        put(out, cap, "{\"ok\":false,\"error\":\"Blackmagic Desktop Video (the DeckLink driver) isn't installed.\"}");
        return 0;
    }
    std::string js = "{\"ok\":true,\"devices\":[";
    IDeckLink* dl = nullptr;
    int index = 0;
    bool first_dev = true;
    while (it->Next(&dl) == S_OK) {
        IDeckLinkOutput* output = nullptr;
        if (dl->QueryInterface(IID_IDeckLinkOutput, (void**)&output) == S_OK) {
            dl_string name = nullptr;
            dl->GetDisplayName(&name);
            dl_bool keying = false;
            IDeckLinkProfileAttributes* attrs = nullptr;
            if (dl->QueryInterface(IID_IDeckLinkProfileAttributes, (void**)&attrs) == S_OK) {
                attrs->GetFlag(BMDDeckLinkSupportsExternalKeying, &keying);
                attrs->Release();
            }
            if (!first_dev) js += ",";
            first_dev = false;
            js += "{\"index\":" + std::to_string(index) + ",\"name\":\"" + json_escape(to_utf8(name)) + "\",\"keying\":" + (keying ? "true" : "false") + ",\"modes\":[";
            IDeckLinkDisplayModeIterator* modes = nullptr;
            bool first_mode = true;
            if (output->GetDisplayModeIterator(&modes) == S_OK) {
                IDeckLinkDisplayMode* m = nullptr;
                while (modes->Next(&m) == S_OK) {
                    dl_bool supported = false;
                    BMDDisplayMode actual;
                    output->DoesSupportVideoMode(bmdVideoConnectionUnspecified, m->GetDisplayMode(), bmdFormat8BitBGRA, bmdNoVideoOutputConversion, bmdSupportedVideoModeDefault, &actual, &supported);
                    if (supported) {
                        BMDTimeValue duration = 0;
                        BMDTimeScale scale = 0;
                        m->GetFrameRate(&duration, &scale);
                        dl_string mname = nullptr;
                        m->GetName(&mname);
                        if (!first_mode) js += ",";
                        first_mode = false;
                        js += "{\"id\":" + std::to_string((uint32_t)m->GetDisplayMode()) + ",\"name\":\"" + json_escape(to_utf8(mname)) + "\",\"width\":" + std::to_string(m->GetWidth()) + ",\"height\":" + std::to_string(m->GetHeight()) + ",\"duration\":" + std::to_string((long long)duration) + ",\"scale\":" + std::to_string((long long)scale) + "}";
                    }
                    m->Release();
                }
                modes->Release();
            }
            js += "]}";
            output->Release();
        }
        dl->Release();
        index++;
    }
    it->Release();
    js += "]}";
    put(out, cap, js);
    return 1;
}

struct KdlOutput {
    IDeckLink* device;
    IDeckLinkOutput* output;
    IDeckLinkKeyer* keyer;
    IDeckLinkMutableVideoFrame* frame;
    int width, height;
};

void* kdl_open(int index, uint32_t mode, int keying, char* err, int errcap) {
    IDeckLink* dl = device_at(index);
    if (!dl) { put(err, errcap, "That DeckLink card isn't connected."); return nullptr; }
    IDeckLinkOutput* output = nullptr;
    if (dl->QueryInterface(IID_IDeckLinkOutput, (void**)&output) != S_OK) {
        dl->Release();
        put(err, errcap, "That DeckLink device has no video output.");
        return nullptr;
    }
    IDeckLinkDisplayMode* dm = nullptr;
    if (output->GetDisplayMode((BMDDisplayMode)mode, &dm) != S_OK || !dm) {
        output->Release(); dl->Release();
        put(err, errcap, "That video format isn't available on this card.");
        return nullptr;
    }
    int w = (int)dm->GetWidth(), h = (int)dm->GetHeight();
    dm->Release();
    if (output->EnableVideoOutput((BMDDisplayMode)mode, bmdVideoOutputFlagDefault) != S_OK) {
        output->Release(); dl->Release();
        put(err, errcap, "The card wouldn't start that output (is another program using it?).");
        return nullptr;
    }
    IDeckLinkKeyer* keyer = nullptr;
    if (keying && dl->QueryInterface(IID_IDeckLinkKeyer, (void**)&keyer) == S_OK) {
        keyer->Enable(true);   // external: fill and key on separate outputs
        keyer->SetLevel(255);
    }
    IDeckLinkMutableVideoFrame* frame = nullptr;
    if (output->CreateVideoFrame(w, h, w * 4, bmdFormat8BitBGRA, bmdFrameFlagDefault, &frame) != S_OK) {
        if (keyer) { keyer->Disable(); keyer->Release(); }
        output->DisableVideoOutput(); output->Release(); dl->Release();
        put(err, errcap, "Couldn't create a video frame on the card.");
        return nullptr;
    }
    KdlOutput* o = new KdlOutput{dl, output, keyer, frame, w, h};
    return o;
}

// Displays one BGRA frame (premultiplied alpha), `width`×`height` matching the mode.
int kdl_frame(void* handle, const uint8_t* bgra, int width, int height) {
    KdlOutput* o = (KdlOutput*)handle;
    if (!o || width != o->width || height != o->height) return 0;
    IDeckLinkVideoBuffer* buf = nullptr;
    if (o->frame->QueryInterface(IID_IDeckLinkVideoBuffer, (void**)&buf) != S_OK) return 0;
    void* bytes = nullptr;
    int ok = 0;
    if (buf->StartAccess(bmdBufferAccessWrite) == S_OK) {
        if (buf->GetBytes(&bytes) == S_OK && bytes) {
            memcpy(bytes, bgra, (size_t)width * height * 4);
            ok = 1;
        }
        buf->EndAccess(bmdBufferAccessWrite);
    }
    buf->Release();
    if (ok && o->output->DisplayVideoFrameSync(o->frame) != S_OK) ok = 0;
    return ok;
}

void kdl_size(void* handle, int* width, int* height) {
    KdlOutput* o = (KdlOutput*)handle;
    if (!o) return;
    *width = o->width;
    *height = o->height;
}

void kdl_close(void* handle) {
    KdlOutput* o = (KdlOutput*)handle;
    if (!o) return;
    if (o->keyer) { o->keyer->Disable(); o->keyer->Release(); }
    o->output->DisableVideoOutput();
    o->frame->Release();
    o->output->Release();
    o->device->Release();
    delete o;
}

}
