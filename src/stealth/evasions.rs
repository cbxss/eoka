//! Evasion scripts to bypass bot detection
//!
//! These scripts are injected before any page content loads to patch
//! detectable browser properties.

use crate::StealthConfig;

/// Core WebDriver evasion - define webdriver=false on prototype (more realistic)
pub const WEBDRIVER_EVASION: &str = r#"
// Define webdriver as false on Navigator.prototype (this is what chaser-oxide does)
// Using false instead of undefined is more realistic for a normal browser
Object.defineProperty(Object.getPrototypeOf(navigator), 'webdriver', {
    get: () => false,
    configurable: true,
    enumerable: true
});

// Also set on Navigator.prototype directly for good measure
try {
    Object.defineProperty(Navigator.prototype, 'webdriver', {
        get: () => false,
        configurable: true,
        enumerable: true
    });
} catch(e) {}

// Remove automation-related properties from window
const automationProps = [
    'callPhantom', '_phantom', 'phantom', '__nightmare', 'domAutomation',
    'domAutomationController', '_selenium', '_Selenium_IDE_Recorder',
    'callSelenium', '__webdriver_script_fn', '__driver_evaluate',
    '__webdriver_evaluate', '__fxdriver_evaluate', '__driver_unwrapped',
    '__webdriver_unwrapped', '__fxdriver_unwrapped', '__selenium_unwrapped',
    '_WEBDRIVER_ELEM_CACHE', 'ChromeDriverw', 'driver-evaluate',
    'webdriver-evaluate', 'selenium-evaluate', 'webdriverCommand',
    'webdriver-evaluate-response', '__webdriverFunc', '__lastWatirAlert',
    '__lastWatirConfirm', '__lastWatirPrompt', '$chrome_asyncScriptInfo',
    '__selenium_evaluate', '__webdriver_script_function'
];

automationProps.forEach(prop => {
    try {
        if (prop in window) delete window[prop];
    } catch(e) {}
});

// Fix for Chrome's runtime.bindingsCalled check
if (window.chrome && window.chrome.runtime) {
    try {
        delete window.chrome.runtime.bindingsCalled;
    } catch(e) {}
}

// Hide automation in Error.stack traces
const originalStackDescriptor = Object.getOwnPropertyDescriptor(Error.prototype, 'stack');
if (originalStackDescriptor && originalStackDescriptor.get) {
    Object.defineProperty(Error.prototype, 'stack', {
        get: function() {
            let stack = originalStackDescriptor.get.call(this);
            if (typeof stack === 'string') {
                stack = stack.split('\n').filter(line =>
                    !line.includes('__puppeteer') &&
                    !line.includes('devtools://') &&
                    !line.includes('chrome-extension://') &&
                    !line.includes('__selenium') &&
                    !line.includes('__webdriver')
                ).join('\n');
            }
            return stack;
        },
        configurable: true
    });
}
"#;

/// CDP marker cleanup - simple and effective approach from chaser-oxide
pub const CDP_EVASION: &str = r#"
// Pattern to match CDP/automation markers
const cdcPattern = /^cdc_|^\$cdc_|^__webdriver|^__selenium|^__driver|^\$chrome_|^\$wdc_/;

// Clean up CDP markers from window
const cleanupWindow = () => {
    for (const prop of Object.getOwnPropertyNames(window)) {
        if (cdcPattern.test(prop)) {
            try { delete window[prop]; } catch(e) {}
        }
    }
};

// Clean up CDP markers from document
const cleanupDocument = () => {
    for (const prop of Object.getOwnPropertyNames(document)) {
        if (cdcPattern.test(prop)) {
            try { delete document[prop]; } catch(e) {}
        }
    }
};

// Run cleanup immediately
cleanupWindow();
cleanupDocument();

// Override Object.getOwnPropertyNames to filter out CDP markers
const originalGetOwnPropertyNames = Object.getOwnPropertyNames;
Object.getOwnPropertyNames = function(obj) {
    const names = originalGetOwnPropertyNames.call(this, obj);
    if (obj === window || obj === document) {
        return names.filter(name => !cdcPattern.test(name));
    }
    return names;
};

// Override Object.keys to filter out CDP markers
const originalKeys = Object.keys;
Object.keys = function(obj) {
    const keys = originalKeys.call(this, obj);
    if (obj === window || obj === document) {
        return keys.filter(key => !cdcPattern.test(key));
    }
    return keys;
};

// Override hasOwnProperty to hide CDP markers
const originalHasOwnProperty = Object.prototype.hasOwnProperty;
Object.prototype.hasOwnProperty = function(prop) {
    if ((this === window || this === document) && cdcPattern.test(prop)) {
        return false;
    }
    return originalHasOwnProperty.call(this, prop);
};

// Override Object.getOwnPropertyDescriptor to hide CDP markers
const originalGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
Object.getOwnPropertyDescriptor = function(obj, prop) {
    if ((obj === window || obj === document) && cdcPattern.test(prop)) {
        return undefined;
    }
    return originalGetOwnPropertyDescriptor.call(this, obj, prop);
};

// Intercept direct property access on document using getter override
const realDocument = document;
try {
    // Create property access interceptor for document
    const documentProxy = new Proxy(realDocument, {
        get(target, prop, receiver) {
            if (typeof prop === 'string' && cdcPattern.test(prop)) {
                return undefined;
            }
            const value = Reflect.get(target, prop, target);
            if (typeof value === 'function') {
                return value.bind(target);
            }
            return value;
        },
        has(target, prop) {
            if (typeof prop === 'string' && cdcPattern.test(prop)) {
                return false;
            }
            return Reflect.has(target, prop);
        },
        getOwnPropertyDescriptor(target, prop) {
            if (typeof prop === 'string' && cdcPattern.test(prop)) {
                return undefined;
            }
            return Reflect.getOwnPropertyDescriptor(target, prop);
        },
        ownKeys(target) {
            return Reflect.ownKeys(target).filter(key =>
                typeof key !== 'string' || !cdcPattern.test(key)
            );
        }
    });

    Object.defineProperty(window, 'document', {
        get: () => documentProxy,
        configurable: true
    });
} catch(e) {
    // Fallback: run cleanup periodically if proxy fails
    setInterval(() => {
        cleanupWindow();
        cleanupDocument();
    }, 100);
}

// Clean up document element attributes
const cleanupDocAttributes = () => {
    const attrs = ['selenium', 'webdriver', 'driver'];
    const docElement = document.documentElement;
    if (docElement) {
        attrs.forEach(attr => {
            try {
                if (docElement.hasAttribute(attr)) {
                    docElement.removeAttribute(attr);
                }
            } catch(e) {}
        });
    }
};

if (document.documentElement) {
    cleanupDocAttributes();
}
document.addEventListener('DOMContentLoaded', cleanupDocAttributes);
"#;

/// Chrome runtime object
pub const CHROME_RUNTIME_EVASION: &str = r#"
window.chrome = {
    runtime: {
        onConnect: {
            addListener: function() {},
            removeListener: function() {}
        },
        onMessage: {
            addListener: function() {},
            removeListener: function() {}
        },
        connect: function() {
            return {
                onMessage: { addListener: function() {} },
                onDisconnect: { addListener: function() {} },
                postMessage: function() {}
            };
        },
        sendMessage: function() {},
        id: undefined
    },
    loadTimes: function() {
        return {
            commitLoadTime: Date.now() / 1000 - Math.random() * 2,
            connectionInfo: "h2",
            finishDocumentLoadTime: Date.now() / 1000 - Math.random(),
            finishLoadTime: Date.now() / 1000 - Math.random() * 0.5,
            firstPaintAfterLoadTime: 0,
            firstPaintTime: Date.now() / 1000 - Math.random() * 1.5,
            navigationType: "Other",
            npnNegotiatedProtocol: "h2",
            requestTime: Date.now() / 1000 - Math.random() * 3,
            startLoadTime: Date.now() / 1000 - Math.random() * 2.5,
            wasAlternateProtocolAvailable: false,
            wasFetchedViaSpdy: true,
            wasNpnNegotiated: true
        };
    },
    csi: function() {
        return {
            onloadT: Date.now(),
            pageT: Math.random() * 1000 + 500,
            startE: Date.now() - Math.random() * 3000,
            tran: 15
        };
    },
    app: {
        isInstalled: false,
        InstallState: { DISABLED: "disabled", INSTALLED: "installed", NOT_INSTALLED: "not_installed" },
        RunningState: { CANNOT_RUN: "cannot_run", READY_TO_RUN: "ready_to_run", RUNNING: "running" }
    }
};
"#;

/// Permissions API consistency fix
pub const PERMISSIONS_EVASION: &str = r#"
// Fix Notification/Permissions API consistency
const originalPermissionsQuery = navigator.permissions.query.bind(navigator.permissions);

try {
    Object.defineProperty(Notification, 'permission', {
        get: () => 'default',
        configurable: true,
        enumerable: true
    });
} catch(e) {}

navigator.permissions.query = function(parameters) {
    const name = parameters.name;

    if (name === 'notifications') {
        return Promise.resolve({
            state: 'prompt',
            name: 'notifications',
            onchange: null,
            addEventListener: function() {},
            removeEventListener: function() {},
            dispatchEvent: function() { return true; }
        });
    }

    if (name === 'clipboard-read' || name === 'clipboard-write') {
        return Promise.resolve({
            state: 'prompt',
            name: name,
            onchange: null,
            addEventListener: function() {},
            removeEventListener: function() {},
            dispatchEvent: function() { return true; }
        });
    }

    return originalPermissionsQuery(parameters).then(function(result) {
        return {
            state: result.state,
            name: result.name || name,
            onchange: result.onchange,
            addEventListener: result.addEventListener?.bind(result) || function() {},
            removeEventListener: result.removeEventListener?.bind(result) || function() {},
            dispatchEvent: result.dispatchEvent?.bind(result) || function() { return true; }
        };
    }).catch(function() {
        return {
            state: 'prompt',
            name: name,
            onchange: null,
            addEventListener: function() {},
            removeEventListener: function() {},
            dispatchEvent: function() { return true; }
        };
    });
};
"#;

/// Plugins spoofing - defined on Navigator.prototype to avoid getOwnPropertyNames detection
pub const PLUGINS_EVASION: &str = r#"
// Define plugins on prototype so Object.getOwnPropertyNames(navigator) returns empty
Object.defineProperty(Navigator.prototype, 'plugins', {
    get: () => {
        const plugins = [
            { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
            { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
            { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' }
        ];

        const pluginArray = Object.create(PluginArray.prototype);
        plugins.forEach((p, i) => {
            const plugin = Object.create(Plugin.prototype);
            Object.defineProperties(plugin, {
                name: { value: p.name },
                filename: { value: p.filename },
                description: { value: p.description },
                length: { value: 1 }
            });
            pluginArray[i] = plugin;
        });

        Object.defineProperty(pluginArray, 'length', { value: plugins.length });
        pluginArray.item = (i) => pluginArray[i];
        pluginArray.namedItem = (name) => plugins.find(p => p.name === name);
        pluginArray.refresh = () => {};

        return pluginArray;
    },
    configurable: true
});
"#;

/// Navigator properties (combined for efficiency)
pub const NAVIGATOR_PROPS_EVASION: &str = r#"
// Language, platform, hardware - all on Navigator.prototype for consistency
const navProps = {
    languages: { get: () => ['en-US', 'en'] },
    language: { get: () => 'en-US' },
    platform: { get: () => 'MacIntel' },
    vendor: { get: () => 'Google Inc.' },
    hardwareConcurrency: { get: () => 8 },
    deviceMemory: { get: () => 8 },
    maxTouchPoints: { get: () => 0 }
};
for (const [prop, desc] of Object.entries(navProps)) {
    Object.defineProperty(Navigator.prototype, prop, { ...desc, configurable: true });
}
"#;

/// Headless detection bypass
pub const HEADLESS_EVASION: &str = r#"
// Fix screen properties
Object.defineProperty(screen, 'availWidth', { get: () => screen.width });
Object.defineProperty(screen, 'availHeight', { get: () => screen.height - 40 });

// Mock window dimensions
Object.defineProperty(window, 'outerWidth', { get: () => window.innerWidth });
Object.defineProperty(window, 'outerHeight', { get: () => window.innerHeight + 85 });

// Fix broken Image detection
const originalImage = window.Image;
window.Image = function(...args) {
    const img = new originalImage(...args);
    Object.defineProperty(img, 'naturalHeight', { get: () => 20 });
    return img;
};
window.Image.prototype = originalImage.prototype;

// Fix window.matchMedia
const originalMatchMedia = window.matchMedia;
if (originalMatchMedia) {
    window.matchMedia = function(query) {
        const result = originalMatchMedia.call(window, query);
        if (query.includes('prefers-reduced-motion')) {
            return {
                matches: false,
                media: query,
                onchange: null,
                addListener: function() {},
                removeListener: function() {},
                addEventListener: function() {},
                removeEventListener: function() {},
                dispatchEvent: function() { return true; }
            };
        }
        return result;
    };
}

// Ensure window.origin is correct
try {
    if (!window.origin || window.origin === 'null') {
        Object.defineProperty(window, 'origin', {
            get: function() {
                return window.location.origin;
            },
            configurable: true
        });
    }
} catch(e) {}
"#;

/// Battery API fix - defined on prototype to avoid getOwnPropertyNames detection
pub const BATTERY_EVASION: &str = r#"
// Define on Navigator.prototype to avoid Object.getOwnPropertyNames(navigator) detection
const originalGetBattery = Navigator.prototype.getBattery;
if (originalGetBattery) {
    Object.defineProperty(Navigator.prototype, 'getBattery', {
        value: function() {
            return originalGetBattery.call(this).then(function(battery) {
                return {
                    charging: true,
                    chargingTime: 0,
                    dischargingTime: Infinity,
                    level: 0.87 + (Math.random() * 0.1),
                    onchargingchange: battery.onchargingchange,
                    onchargingtimechange: battery.onchargingtimechange,
                    ondischargingtimechange: battery.ondischargingtimechange,
                    onlevelchange: battery.onlevelchange,
                    addEventListener: battery.addEventListener?.bind(battery) || function() {},
                    removeEventListener: battery.removeEventListener?.bind(battery) || function() {},
                    dispatchEvent: battery.dispatchEvent?.bind(battery) || function() { return true; }
                };
            }).catch(function() {
                return {
                    charging: true,
                    chargingTime: 0,
                    dischargingTime: Infinity,
                    level: 0.91,
                    onchargingchange: null,
                    onchargingtimechange: null,
                    ondischargingtimechange: null,
                    onlevelchange: null,
                    addEventListener: function() {},
                    removeEventListener: function() {},
                    dispatchEvent: function() { return true; }
                };
            });
        },
        configurable: true,
        writable: true
    });
}
"#;

/// Navigator connection fix
pub const NAVIGATOR_EXTRA_EVASION: &str = r#"
// Fix navigator.userAgentData
if (navigator.userAgentData) {
    const originalGetHighEntropyValues = navigator.userAgentData.getHighEntropyValues?.bind(navigator.userAgentData);
    if (originalGetHighEntropyValues) {
        navigator.userAgentData.getHighEntropyValues = function(hints) {
            return originalGetHighEntropyValues(hints).then(function(data) {
                if (data.brands) {
                    data.brands = data.brands.filter(b =>
                        !b.brand.toLowerCase().includes('headless')
                    );
                }
                return data;
            });
        };
    }
}

// Fix navigator.connection
if (navigator.connection) {
    try {
        const connectionProps = {
            downlink: 10,
            effectiveType: '4g',
            rtt: 50,
            saveData: false
        };

        for (const [key, value] of Object.entries(connectionProps)) {
            try {
                Object.defineProperty(navigator.connection, key, {
                    get: () => value,
                    configurable: true,
                    enumerable: true
                });
            } catch(e) {}
        }
    } catch(e) {}
}
"#;

/// Combined fingerprint evasion (WebGL + Canvas + Audio)
pub const FINGERPRINT_EVASION: &str = r#"
// WebGL vendor/renderer spoofing
const spoofWebGL = (proto) => {
    const orig = proto.getParameter;
    proto.getParameter = function(p) {
        if (p === 37445) return 'Intel Inc.';
        if (p === 37446) return 'Intel Iris Pro Graphics 6200';
        return orig.call(this, p);
    };
};
spoofWebGL(WebGLRenderingContext.prototype);
if (typeof WebGL2RenderingContext !== 'undefined') spoofWebGL(WebGL2RenderingContext.prototype);

// Canvas noise
const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
HTMLCanvasElement.prototype.toDataURL = function(type) {
    if (!type || type === 'image/png') {
        const ctx = this.getContext('2d');
        if (ctx) {
            const d = ctx.getImageData(0, 0, this.width, this.height);
            for (let i = 0; i < d.data.length; i += 4) d.data[i] ^= (Math.random() * 2) | 0;
            ctx.putImageData(d, 0, 0);
        }
    }
    return origToDataURL.apply(this, arguments);
};

// Audio noise
const origGetChannelData = AudioBuffer.prototype.getChannelData;
AudioBuffer.prototype.getChannelData = function(ch) {
    const d = origGetChannelData.call(this, ch);
    for (let i = 0; i < d.length; i += 100) d[i] += Math.random() * 0.0001 - 0.00005;
    return d;
};
"#;

/// WebRTC leak protection - prevent real IP from leaking
pub const WEBRTC_EVASION: &str = r#"
// Disable WebRTC IP leak by overriding RTCPeerConnection
if (typeof RTCPeerConnection !== 'undefined') {
    const origRTCPeerConnection = RTCPeerConnection;

    // Override to prevent IP leak via STUN
    window.RTCPeerConnection = function(config, constraints) {
        // Filter out Google STUN servers which are commonly used for IP detection
        if (config && config.iceServers) {
            config.iceServers = config.iceServers.filter(server => {
                const urls = Array.isArray(server.urls) ? server.urls : [server.urls];
                return !urls.some(url =>
                    url.includes('stun.l.google.com') ||
                    url.includes('stun1.l.google.com') ||
                    url.includes('stun2.l.google.com')
                );
            });
        }
        return new origRTCPeerConnection(config, constraints);
    };

    // Copy prototype
    window.RTCPeerConnection.prototype = origRTCPeerConnection.prototype;

    // Preserve static methods
    Object.keys(origRTCPeerConnection).forEach(key => {
        window.RTCPeerConnection[key] = origRTCPeerConnection[key];
    });
}

// Also handle webkitRTCPeerConnection
if (typeof webkitRTCPeerConnection !== 'undefined') {
    window.webkitRTCPeerConnection = window.RTCPeerConnection;
}
"#;

/// Speech synthesis - headless Chrome has 0 voices, real Chrome has many
pub const SPEECH_EVASION: &str = r#"
// Spoof speechSynthesis.getVoices() to return realistic voices
if (typeof speechSynthesis !== 'undefined') {
    const defaultVoices = [
        { name: 'Alex', lang: 'en-US', localService: true, default: true, voiceURI: 'Alex' },
        { name: 'Samantha', lang: 'en-US', localService: true, default: false, voiceURI: 'Samantha' },
        { name: 'Victoria', lang: 'en-US', localService: true, default: false, voiceURI: 'Victoria' },
        { name: 'Daniel', lang: 'en-GB', localService: true, default: false, voiceURI: 'Daniel' },
        { name: 'Google US English', lang: 'en-US', localService: false, default: false, voiceURI: 'Google US English' }
    ].map(v => {
        const voice = Object.create(SpeechSynthesisVoice.prototype);
        Object.defineProperties(voice, {
            name: { value: v.name, enumerable: true },
            lang: { value: v.lang, enumerable: true },
            localService: { value: v.localService, enumerable: true },
            default: { value: v.default, enumerable: true },
            voiceURI: { value: v.voiceURI, enumerable: true }
        });
        return voice;
    });

    const origGetVoices = speechSynthesis.getVoices.bind(speechSynthesis);
    speechSynthesis.getVoices = function() {
        const voices = origGetVoices();
        return voices.length > 0 ? voices : defaultVoices;
    };
}
"#;

/// Media devices - headless returns empty array
pub const MEDIA_DEVICES_EVASION: &str = r#"
// Spoof navigator.mediaDevices.enumerateDevices()
if (navigator.mediaDevices && navigator.mediaDevices.enumerateDevices) {
    const origEnumerateDevices = navigator.mediaDevices.enumerateDevices.bind(navigator.mediaDevices);

    navigator.mediaDevices.enumerateDevices = async function() {
        const devices = await origEnumerateDevices();
        if (devices.length === 0) {
            // Return fake devices if none exist (headless mode)
            return [
                { deviceId: 'default', groupId: 'default', kind: 'audioinput', label: '' },
                { deviceId: 'default', groupId: 'default', kind: 'audiooutput', label: '' },
                { deviceId: 'default', groupId: 'default', kind: 'videoinput', label: '' }
            ].map(d => {
                const device = Object.create(MediaDeviceInfo.prototype);
                Object.defineProperties(device, {
                    deviceId: { value: d.deviceId, enumerable: true },
                    groupId: { value: d.groupId, enumerable: true },
                    kind: { value: d.kind, enumerable: true },
                    label: { value: d.label, enumerable: true },
                    toJSON: { value: () => d }
                });
                return device;
            });
        }
        return devices;
    };
}
"#;

/// Bluetooth API - headless throws error, real Chrome returns object
pub const BLUETOOTH_EVASION: &str = r#"
// Fix navigator.bluetooth which errors in headless
if (!navigator.bluetooth) {
    Object.defineProperty(Navigator.prototype, 'bluetooth', {
        get: () => ({
            getAvailability: () => Promise.resolve(false),
            requestDevice: () => Promise.reject(new DOMException('User cancelled', 'NotFoundError')),
            getDevices: () => Promise.resolve([]),
            addEventListener: () => {},
            removeEventListener: () => {},
            dispatchEvent: () => true
        }),
        configurable: true
    });
}
"#;

/// Timezone consistency - ensure Date timezone matches claimed location
pub const TIMEZONE_EVASION: &str = r#"
// Ensure Intl.DateTimeFormat returns consistent timezone
const targetTimezone = 'America/Los_Angeles';
const origDateTimeFormat = Intl.DateTimeFormat;

Intl.DateTimeFormat = function(locales, options) {
    if (!options) options = {};
    if (!options.timeZone) options.timeZone = targetTimezone;
    return new origDateTimeFormat(locales, options);
};
Intl.DateTimeFormat.prototype = origDateTimeFormat.prototype;
Intl.DateTimeFormat.supportedLocalesOf = origDateTimeFormat.supportedLocalesOf;

// Override Date.prototype.getTimezoneOffset to match
// PST/PDT is UTC-8/UTC-7, so offset is 480/420 minutes
const origGetTimezoneOffset = Date.prototype.getTimezoneOffset;
Date.prototype.getTimezoneOffset = function() {
    // Check if we're in DST (rough approximation)
    const month = this.getMonth();
    const isDST = month >= 2 && month <= 10; // March to November
    return isDST ? 420 : 480; // PDT: -7, PST: -8
};
"#;

/// Build the complete evasion script based on config
pub fn build_evasion_script(config: &StealthConfig) -> String {
    let mut scripts = vec![
        WEBDRIVER_EVASION,
        CDP_EVASION,
        CHROME_RUNTIME_EVASION,
        PERMISSIONS_EVASION,
        PLUGINS_EVASION,
        NAVIGATOR_PROPS_EVASION,  // Combined: languages, platform, hardware, etc.
        HEADLESS_EVASION,
        BATTERY_EVASION,
        NAVIGATOR_EXTRA_EVASION,
        // New evasions for better coverage
        WEBRTC_EVASION,
        SPEECH_EVASION,
        MEDIA_DEVICES_EVASION,
        BLUETOOTH_EVASION,
        TIMEZONE_EVASION,
    ];

    // Add fingerprint evasion if any spoofing enabled
    if config.webgl_spoof || config.canvas_spoof || config.audio_spoof {
        scripts.push(FINGERPRINT_EVASION);
    }

    // Wrap in IIFE
    format!("(function(){{{}}})();", scripts.join("\n"))
}

/// Get the full evasion script (all options enabled)
pub fn full_evasion_script() -> String {
    build_evasion_script(&StealthConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_full_script() {
        let config = StealthConfig::default();
        let script = build_evasion_script(&config);
        assert!(script.contains("webdriver"));
        assert!(script.contains("WebGLRenderingContext"));
        assert!(script.contains("HTMLCanvasElement"));
        assert!(script.contains("AudioBuffer"));
    }

    #[test]
    fn test_script_is_wrapped_in_iife() {
        let config = StealthConfig::default();
        let script = build_evasion_script(&config);
        assert!(script.starts_with("(function()"));
        assert!(script.ends_with("})();"));
    }
}
