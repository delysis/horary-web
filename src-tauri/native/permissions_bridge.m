#import <AVFoundation/AVFoundation.h>
#import <dispatch/dispatch.h>

// Called on an owned background worker. The OS owns consent and remembers its
// decision; the application never stores a separate refusal preference.
int horary_microphone_permission(int request) {
    if (request) {
        dispatch_semaphore_t finished = dispatch_semaphore_create(0);
        [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
            (void)granted;
            dispatch_semaphore_signal(finished);
        }];
        dispatch_semaphore_wait(finished, dispatch_time(DISPATCH_TIME_NOW, 120 * NSEC_PER_SEC));
    }
    return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}
