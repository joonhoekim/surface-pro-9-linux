// SPDX-License-Identifier: MIT
/*
 * cam-ctrl-delay: how many frames a sensor control takes to show in the image.
 *
 * usage: cam-ctrl-delay VIDEO_NODE SUBDEV_NODE CONTROL_ID A B [WAIT_US]
 *
 * The pipeline must be set up first (cam-raw.sh does it; its capture leaves the formats in place).
 * Streams 40 10-bit unpacked frames, writes value A before the start, B WAIT_US (default 0) after
 * frame 15 is dequeued and A again after frame 27, and prints the mean of every frame.
 * A buffer is dequeued at the end of its frame, shortly before the next frame starts. libcamera writes
 * controls on the frame start event, so its delay is measured with WAIT_US past the vertical blanking
 * (5000 is enough at 30 fps): the write then falls into frame K+1, and when frame M is the first to
 * show it, the delay for libcamera's sensor properties is M - (K + 1). With WAIT_US 0 the write lands
 * before the start of frame K+1 and shows one frame earlier when the sensor latches at frame start.
 * CONTROL_ID: 0x00980911 exposure, 0x009e0903 analogue gain, 0x009e0901 vblank.
 *
 * Build: cc -O2 -o cam-ctrl-delay cam-ctrl-delay.c
 */
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>
#include <linux/videodev2.h>

#define NBUF 4

static void die(const char *what)
{
	perror(what);
	exit(1);
}

static void set_ctrl(int fd, uint32_t id, int32_t value)
{
	struct v4l2_control c = { .id = id, .value = value };

	if (ioctl(fd, VIDIOC_S_CTRL, &c) < 0)
		die("VIDIOC_S_CTRL");
}

int main(int argc, char **argv)
{
	if (argc < 6) {
		fprintf(stderr, "usage: %s VIDEO SUBDEV CONTROL_ID A B [WAIT_US]\n", argv[0]);
		return 2;
	}

	int vfd = open(argv[1], O_RDWR);
	int sfd = open(argv[2], O_RDWR);
	uint32_t id = strtoul(argv[3], NULL, 0);
	int32_t a = atoi(argv[4]), b = atoi(argv[5]);
	int frames = 40;
	useconds_t wait_us = argc > 6 ? atoi(argv[6]) : 0;
	if (vfd < 0 || sfd < 0)
		die("open");

	struct v4l2_format fmt = { .type = V4L2_BUF_TYPE_VIDEO_CAPTURE };
	if (ioctl(vfd, VIDIOC_G_FMT, &fmt) < 0)
		die("VIDIOC_G_FMT");
	unsigned int w = fmt.fmt.pix.width, h = fmt.fmt.pix.height, stride = fmt.fmt.pix.bytesperline;

	struct v4l2_requestbuffers req = { .count = NBUF, .type = V4L2_BUF_TYPE_VIDEO_CAPTURE, .memory = V4L2_MEMORY_MMAP };
	if (ioctl(vfd, VIDIOC_REQBUFS, &req) < 0)
		die("VIDIOC_REQBUFS");

	void *map[NBUF];
	for (unsigned int i = 0; i < req.count; i++) {
		struct v4l2_buffer buf = { .type = req.type, .memory = req.memory, .index = i };
		if (ioctl(vfd, VIDIOC_QUERYBUF, &buf) < 0)
			die("VIDIOC_QUERYBUF");
		map[i] = mmap(NULL, buf.length, PROT_READ, MAP_SHARED, vfd, buf.m.offset);
		if (map[i] == MAP_FAILED)
			die("mmap");
		if (ioctl(vfd, VIDIOC_QBUF, &buf) < 0)
			die("VIDIOC_QBUF");
	}

	set_ctrl(sfd, id, a);
	int type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
	if (ioctl(vfd, VIDIOC_STREAMON, &type) < 0)
		die("VIDIOC_STREAMON");

	for (int n = 0; n < frames; n++) {
		struct v4l2_buffer buf = { .type = req.type, .memory = req.memory };
		const char *note = "";

		if (ioctl(vfd, VIDIOC_DQBUF, &buf) < 0)
			die("VIDIOC_DQBUF");
		if (n == 15 || n == 27)
			usleep(wait_us);
		if (n == 15) {
			set_ctrl(sfd, id, b);
			note = " <- B written";
		} else if (n == 27) {
			set_ctrl(sfd, id, a);
			note = " <- A written";
		}

		const uint16_t *p = map[buf.index];
		uint64_t top = 0, bottom = 0;
		unsigned int count = 0;
		for (unsigned int y = h / 16; y < h / 8; y += 2)
			for (unsigned int x = 0; x < w; x += 8, count++) {
				top += p[y * (stride / 2) + x];
				bottom += p[(h - 1 - y) * (stride / 2) + x];
			}
		printf("frame %2d seq %u ts %ld.%06ld top %.1f bottom %.1f%s%s\n", n, buf.sequence,
		       (long)buf.timestamp.tv_sec, (long)buf.timestamp.tv_usec,
		       (double)top / count, (double)bottom / count,
		       buf.flags & V4L2_BUF_FLAG_ERROR ? " ERROR" : "", note);
		if (ioctl(vfd, VIDIOC_QBUF, &buf) < 0)
			die("VIDIOC_QBUF");
	}

	ioctl(vfd, VIDIOC_STREAMOFF, &type);
	return 0;
}
